//! Data index management.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::anyhow;
use dashmap::DashMap;
use tokio::sync::RwLock;

use crate::auth::{Claims, User, UserRole};
use crate::error::{AppError, ShutdownError};
use crate::models::placement::{ControlGroup, ControlGroupObjectRef, PipelineReplica, StreamReplica};
use crate::models::prelude::*;
use crate::models::schema;
use crate::models::WithId;

const ERR_INDEX_MISSING_NS: &str = "namespace not found in index, this is a bug, please open an issue";

/// The CRC's data index.
///
/// It is important to note that we must reduce lock contention as much as possible. This can
/// easily be achieved by simply dropping the lock guard as soon as possible whenever a lock is
/// taken, especially given that all internal data is Arc'd and can be easily cloned, allowing the
/// lock to be dropped very quickly.
#[derive(Default)]
pub struct CRCIndex {
    /// Index of all managed schema branches which have been applied to the system.
    pub schema_branches: DashMap<String, i64>,

    /// Index of namespaces in the system by ID.
    pub namespaces: DashMap<u64, Arc<WithId<schema::Namespace>>>,
    /// Index of namespace names to IDs.
    pub namespace_names: DashMap<String, u64>,

    /// Index of endpoints by ID.
    pub endpoints: DashMap<u64, ()>,
    /// Index of endpoint names (hashed namespace/name) to ID.
    pub endpoint_names: DashMap<u64, u64>,

    /// Index of streams by ID.
    pub streams: DashMap<u64, Arc<WithId<schema::Stream>>>,
    /// Index of stream names (hashed namespace/name) to ID.
    pub stream_names: DashMap<u64, u64>,

    /// Index of pipelines by ID.
    pub pipelines: DashMap<u64, Arc<WithId<schema::Pipeline>>>,
    /// Index of pipeline names (hashed namespace/name) to ID.
    pub pipeline_names: DashMap<u64, u64>,

    /// Index of control groups.
    pub control_groups: DashMap<u64, Arc<ControlGroup>>,
    /// Index of stream partition control groups, index by (stream, partition),
    /// pointing to control group ID.
    pub stream_control_groups: DashMap<(u64, u32), u64>,
    /// Index of pipeline control groups, index by pipeline, pointing to control group ID.
    pub pipeline_control_groups: DashMap<u64, u64>,

    /// Index of stream replicas.
    pub stream_replicas: DashMap<u64, Arc<StreamReplica>>,
    /// Index of pipeline replicas.
    pub pipeline_replicas: DashMap<u64, Arc<PipelineReplica>>,

    /// Index of users.
    pub users: DashMap<String, Arc<UserRole>>,
    /// Index of tokens.
    pub tokens: DashMap<u64, Arc<Claims>>,
}

impl CRCIndex {
    /// Create a new instance.
    pub fn new(
        users: Vec<User>, tokens: Vec<(u64, Claims)>, namespaces: Vec<Arc<WithId<schema::Namespace>>>, streams: Vec<Arc<WithId<schema::Stream>>>,
        pipelines: Vec<Arc<WithId<schema::Pipeline>>>, schema_branches: Vec<schema::SchemaBranch>, control_groups: Vec<Arc<ControlGroup>>,
        stream_replicas: Vec<Arc<StreamReplica>>, pipeline_replicas: Vec<Arc<PipelineReplica>>,
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
        for val in namespaces {
            index.namespace_names.insert(val.model.name.clone(), val.id);
            index.namespaces.insert(val.id, val);
        }
        for val in streams {
            let hash = seahash::hash(val.model.namespaced_name().as_bytes());
            index.stream_names.insert(hash, val.id);
            index.streams.insert(val.id, val);
        }
        for val in pipelines {
            let hash = seahash::hash(val.model.namespaced_name().as_bytes());
            index.pipeline_names.insert(hash, val.id);
            index.pipelines.insert(val.id, val);
        }
        for val in control_groups {
            match val.object_ref {
                ControlGroupObjectRef::Stream { id, partition } => {
                    index.stream_control_groups.insert((id, partition), val.id);
                }
                ControlGroupObjectRef::Pipeline { id } => {
                    index.pipeline_control_groups.insert(id, val.id);
                }
            }
            index.control_groups.insert(val.id, val);
        }
        for val in stream_replicas {
            index.stream_replicas.insert(val.id, val);
        }
        for val in pipeline_replicas {
            index.pipeline_replicas.insert(val.id, val);
        }

        index
    }

    /// Fully replace the contents of this index with the given new index.
    pub(super) fn replace(&self, new: Self) {
        self.schema_branches.clear();
        new.schema_branches.into_iter().for_each(|(key, val)| {
            let _ = self.schema_branches.insert(key, val);
        });

        self.namespace_names.clear();
        new.namespace_names.into_iter().for_each(|(key, val)| {
            let _ = self.namespace_names.insert(key, val);
        });
        self.namespaces.clear();
        new.namespaces.into_iter().for_each(|(key, val)| {
            let _ = self.namespaces.insert(key, val);
        });

        self.stream_names.clear();
        new.stream_names.into_iter().for_each(|(key, val)| {
            let _ = self.stream_names.insert(key, val);
        });
        self.streams.clear();
        new.streams.into_iter().for_each(|(key, val)| {
            let _ = self.streams.insert(key, val);
        });

        self.pipeline_names.clear();
        new.pipeline_names.into_iter().for_each(|(key, val)| {
            let _ = self.pipeline_names.insert(key, val);
        });
        self.pipelines.clear();
        new.pipelines.into_iter().for_each(|(key, val)| {
            let _ = self.pipelines.insert(key, val);
        });

        self.users.clear();
        new.users.into_iter().for_each(|(key, val)| {
            let _ = self.users.insert(key, val);
        });

        self.tokens.clear();
        new.tokens.into_iter().for_each(|(key, val)| {
            let _ = self.tokens.insert(key, val);
        });

        self.control_groups.clear();
        new.control_groups.into_iter().for_each(|(key, val)| {
            let _ = self.control_groups.insert(key, val);
        });
        self.stream_control_groups.clear();
        new.stream_control_groups.into_iter().for_each(|(key, val)| {
            let _ = self.stream_control_groups.insert(key, val);
        });
        self.pipeline_control_groups.clear();
        new.pipeline_control_groups.into_iter().for_each(|(key, val)| {
            let _ = self.pipeline_control_groups.insert(key, val);
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
                IndexWriteOp::InsertNamespace(ns) => {
                    self.namespace_names.insert(ns.model.name.clone(), ns.id);
                    self.namespaces.insert(ns.id, ns);
                }
                IndexWriteOp::InsertStream(stream) => {
                    let hash = seahash::hash(stream.model.namespaced_name().as_bytes());
                    self.stream_names.insert(hash, stream.id);
                    self.streams.insert(stream.id, stream);
                }
                IndexWriteOp::InsertPipeline(pipeline) => {
                    let hash = seahash::hash(pipeline.model.namespaced_name().as_bytes());
                    self.pipeline_names.insert(hash, pipeline.id);
                    self.pipelines.insert(pipeline.id, pipeline);
                }
                IndexWriteOp::InsertControlGroup(cg) => {
                    match cg.object_ref {
                        ControlGroupObjectRef::Stream { id, partition } => {
                            self.stream_control_groups.insert((id, partition), cg.id);
                        }
                        ControlGroupObjectRef::Pipeline { id } => {
                            self.pipeline_control_groups.insert(id, cg.id);
                        }
                    }
                    self.control_groups.insert(cg.id, cg);
                }
                IndexWriteOp::UpdateSchemaBranch { branch, timestamp } => {
                    self.schema_branches.insert(branch, timestamp);
                }
                IndexWriteOp::InsertStreamReplica(replica) => {
                    self.stream_replicas.insert(replica.id, replica);
                }
                IndexWriteOp::InsertPipelineReplica(replica) => {
                    self.pipeline_replicas.insert(replica.id, replica);
                }
            }
        }
        Ok(())
    }

    /// Get a namespace by its name.
    pub fn get_namespace(&self, ns: &str) -> Option<Arc<WithId<schema::Namespace>>> {
        match self.namespace_names.get(ns).map(|val| *val.value()) {
            Some(id) => self.namespaces.get(&id).map(|val| val.value().clone()),
            None => None,
        }
    }

    pub fn get_stream(&self, ns: &str, stream: &str) -> Option<Arc<WithId<schema::Stream>>> {
        let hash = seahash::hash(format!("{}/{}", ns, stream).as_bytes());
        match self.stream_names.get(&hash).map(|val| *val.value()) {
            Some(id) => self.streams.get(&id).map(|val| val.value().clone()),
            None => None,
        }
    }

    pub fn get_pipeline(&self, ns: &str, pipeline: &str) -> Option<Arc<WithId<schema::Pipeline>>> {
        let hash = seahash::hash(format!("{}/{}", ns, pipeline).as_bytes());
        match self.pipeline_names.get(&hash).map(|val| *val.value()) {
            Some(id) => self.pipelines.get(&id).map(|val| val.value().clone()),
            None => None,
        }
    }

    /// Get the given token's claims, else return an auth error.
    pub fn must_get_token_claims(&self, token_id: &u64) -> anyhow::Result<Arc<Claims>> {
        match self.tokens.get(token_id).map(|val| val.value().clone()) {
            Some(claims) => Ok(claims),
            None => Err(AppError::UnknownToken.into()),
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
    InsertNamespace(Arc<WithId<schema::Namespace>>),
    /// Insert a new stream record into the index.
    InsertStream(Arc<WithId<schema::Stream>>),
    /// Insert a new pipeline record into the index.
    InsertPipeline(Arc<WithId<schema::Pipeline>>),
    /// Insert a new control group record into the index.
    InsertControlGroup(Arc<ControlGroup>),
    /// Update a schema branch with a new timestamp.
    UpdateSchemaBranch { branch: String, timestamp: i64 },
    /// Insert a new stream replica record into the index.
    InsertStreamReplica(Arc<StreamReplica>),
    /// Insert a new pipeline replica record into the index.
    InsertPipelineReplica(Arc<PipelineReplica>),
}
