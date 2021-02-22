//! Data index management.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::anyhow;
use dashmap::DashMap;
use tokio::sync::RwLock;

use crate::auth::{Claims, User, UserRole};
use crate::ctl_raft::storage::ShutdownError;
use crate::models::placement::{PipelineReplica, StreamReplica};
use crate::models::prelude::*;
use crate::models::schema;

const ERR_INDEX_MISSING_NS: &str = "namespace not found in index, this is a bug, please open an issue";

/// The system data index.
///
/// Each individual field of the index must be independently locked for reading/writing. It is
/// important to note that we must reduce lock contention as much as possible. This can easily
/// be achieved by simply dropping the lock guard as soon as possible whenever a lock is taken,
/// especially given that all internal data is Arc'd and can be easily cloned, allowing the lock
/// to be dropped very quickly.
#[derive(Default)]
pub struct HCoreIndex {
    /// Index of all managed schema branches which have been applied to the system.
    pub schema_branches: DashMap<String, i64>,
    /// Index of all live namespaces in the system.
    pub namespaces: DashMap<String, Arc<NamespaceIndex>>,
    /// Index of all stream replicas.
    pub stream_replicas: DashMap<u64, Arc<StreamReplica>>,
    /// Index of all pipeline replicas.
    pub pipeline_replicas: DashMap<u64, Arc<PipelineReplica>>,
    /// Index of data for users data.
    pub users: DashMap<String, Arc<UserRole>>,
    /// Index of data for tokens data.
    pub tokens: DashMap<u64, Arc<Claims>>,
}

impl HCoreIndex {
    /// Create a new instance.
    pub fn new(
        users: Vec<User>, tokens: Vec<(u64, Claims)>, namespaces: Vec<schema::Namespace>, streams: Vec<schema::Stream>,
        pipelines: Vec<schema::Pipeline>, schema_branches: Vec<schema::SchemaBranch>,
    ) -> Self {
        let index = Self::default();
        for val in schema_branches {
            let _ = index.schema_branches.insert(val.name, val.timestamp);
        }
        for val in users {
            let _ = index.users.insert(val.name, Arc::new(val.role));
        }
        for (key, val) in tokens {
            let _ = index.tokens.insert(key, Arc::new(val));
        }

        // Build up namespaces data.
        for val in namespaces {
            index.namespaces.insert(val.name, Arc::new(NamespaceIndex::new(val.description)));
        }
        for val in streams {
            let ns = index
                .namespaces
                .entry(val.namespace().into())
                .or_insert_with(|| Arc::new(NamespaceIndex::new(String::new())));
            ns.value().streams.insert(val.name().into(), Arc::new(val));
        }
        for val in pipelines {
            let ns = index
                .namespaces
                .entry(val.namespace().into())
                .or_insert_with(|| Arc::new(NamespaceIndex::new(String::new())));
            ns.value().pipelines.insert(val.name().into(), Arc::new(val));
        }

        index
    }

    /// Fully replace the contents of this index with the given new index.
    pub(super) fn replace(&self, new: Self) {
        self.schema_branches.clear();
        new.schema_branches.into_iter().for_each(|(key, val)| {
            let _ = self.schema_branches.insert(key, val);
        });
        self.namespaces.clear();
        new.namespaces.into_iter().for_each(|(key, val)| {
            let _ = self.namespaces.insert(key, val);
        });
        self.users.clear();
        new.users.into_iter().for_each(|(key, val)| {
            let _ = self.users.insert(key, val);
        });
        self.tokens.clear();
        new.tokens.into_iter().for_each(|(key, val)| {
            let _ = self.tokens.insert(key, val);
        });
        self.stream_replicas.clear();
        new.stream_replicas.into_iter().for_each(|(key, val)| {
            let _ = self.stream_replicas.insert(key, val);
        });
        self.pipeline_replicas.clear();
        new.pipeline_replicas.into_iter().for_each(|(key, val)| {
            let _ = self.pipeline_replicas.insert(key, val);
        });
    }

    pub(super) async fn apply_batch(&self, ops: IndexWriteBatch) -> Result<(), ShutdownError> {
        for op in ops.ops {
            match op {
                IndexWriteOp::InsertNamespace { name, description } => {
                    let ns = NamespaceIndex::new(description);
                    self.namespaces.insert(name, Arc::new(ns));
                }
                IndexWriteOp::InsertStream { stream } => {
                    let ns = self
                        .get_namespace(stream.namespace())
                        .ok_or_else(|| ShutdownError(anyhow!(ERR_INDEX_MISSING_NS)))?;
                    ns.streams.insert(stream.name().into(), Arc::new(stream));
                }
                IndexWriteOp::InsertPipeline { pipeline } => {
                    let ns = self
                        .get_namespace(pipeline.namespace())
                        .ok_or_else(|| ShutdownError(anyhow!(ERR_INDEX_MISSING_NS)))?;
                    ns.pipelines.insert(pipeline.name().into(), Arc::new(pipeline));
                }
                IndexWriteOp::UpdateSchemaBranch { branch, timestamp } => {
                    self.schema_branches.insert(branch, timestamp);
                }
                IndexWriteOp::InsertStreamReplica { replica } => {
                    self.stream_replicas.insert(replica.id, replica);
                }
                IndexWriteOp::InsertPipelineReplica { replica } => {
                    self.pipeline_replicas.insert(replica.id, replica);
                }
            }
        }
        Ok(())
    }

    pub fn get_namespace(&self, ns: &str) -> Option<Arc<NamespaceIndex>> {
        self.namespaces.get(ns).map(|val| val.value().clone())
    }

    pub fn get_stream(&self, ns: &str, stream: &str) -> Option<Arc<schema::Stream>> {
        let ns = self.get_namespace(ns)?;
        let meta_opt = ns.streams.get(stream).map(|val| val.value().clone());
        meta_opt
    }

    pub fn get_pipeline(&self, ns: &str, pipeline: &str) -> Option<Arc<schema::Pipeline>> {
        let ns = self.get_namespace(ns)?;
        let meta_opt = ns.pipelines.get(pipeline).map(|val| val.value().clone());
        meta_opt
    }
}

/// An index structure used to track the resources of a namespace.
#[derive(Default)]
pub struct NamespaceIndex {
    /// A description of this namespace.
    pub description: String,
    /// Index of endpoints in this namespace.
    pub endpoints: DashMap<String, ()>,
    /// Index of streams in this namespace.
    pub streams: DashMap<String, Arc<schema::Stream>>,
    /// Index of pipelines in this namespace.
    pub pipelines: DashMap<String, Arc<schema::Pipeline>>,
}

impl NamespaceIndex {
    /// Create a new instance.
    pub fn new(description: String) -> Self {
        NamespaceIndex {
            description,
            ..Default::default()
        }
    }
}

/// A batch of operations to apply to the index atomically.
#[derive(Default)]
pub(super) struct IndexWriteBatch {
    ops: Vec<IndexWriteOp>,
}

impl IndexWriteBatch {
    /// Push a new operation into this write batch.
    pub(super) fn push(&mut self, op: IndexWriteOp) {
        self.ops.push(op);
    }
}

#[allow(clippy::enum_variant_names)]
pub(super) enum IndexWriteOp {
    /// Insert a new namespace record into the index.
    InsertNamespace { name: String, description: String },
    /// Insert a new stream record into the index.
    InsertStream { stream: schema::Stream },
    /// Insert a new pipeline record into the index.
    InsertPipeline { pipeline: schema::Pipeline },
    /// Update a schema branch with a new timestamp.
    UpdateSchemaBranch { branch: String, timestamp: i64 },
    /// Insert a new stream replica record into the index.
    InsertStreamReplica { replica: Arc<StreamReplica> },
    /// Insert a new pipeline replica record into the index.
    InsertPipelineReplica { replica: Arc<PipelineReplica> },
}
