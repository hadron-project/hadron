//! Data index management.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::anyhow;
use tokio::sync::RwLock;

use crate::auth::{Claims, UserRole};
use crate::core::storage::ShutdownError;

const ERR_INDEX_MISSING_NS: &str = "namespace not found in index, this is a bug, please open an issue";

/// The system data index.
///
/// Each individual field of the index must be independently locked for reading/writing. It is
/// important to note that we must reduce lock contention as much as possible. This can easily
/// be achieved by simply dropping the lock guard as soon as possible whenever a lock is taken.
pub struct HCoreIndex {
    /// Index of all changesets which have been applied to the system.
    pub changesets: RwLock<HashMap<String, ()>>,
    /// Index of all live namespaces in the system.
    pub namespaces: RwLock<HashMap<String, Arc<NamespaceIndex>>>,
    /// Index of data for users data.
    pub users: RwLock<HashMap<String, Arc<UserRole>>>,
    /// Index of data for tokens data.
    pub tokens: RwLock<HashMap<u64, Arc<Claims>>>,
}

impl HCoreIndex {
    pub(super) async fn apply_batch(&self, ops: IndexWriteBatch) -> Result<(), ShutdownError> {
        for op in ops.ops {
            match op {
                IndexWriteOp::InsertNamespace { name, description } => {
                    let mut ns = NamespaceIndex::default();
                    ns.description = description;
                    self.namespaces.write().await.insert(name, Arc::new(ns));
                }
                IndexWriteOp::InsertStream { namespace, name, meta } => {
                    let ns = self
                        .get_namespace(&namespace)
                        .await
                        .ok_or_else(|| ShutdownError(anyhow!(ERR_INDEX_MISSING_NS)))?;
                    ns.streams.write().await.insert(name, Arc::new(meta));
                }
                IndexWriteOp::InsertPipeline { namespace, name, meta } => {
                    let ns = self
                        .get_namespace(&namespace)
                        .await
                        .ok_or_else(|| ShutdownError(anyhow!(ERR_INDEX_MISSING_NS)))?;
                    ns.pipelines.write().await.insert(name, Arc::new(meta));
                }
            }
        }
        Ok(())
    }

    pub async fn get_namespace(&self, ns: &str) -> Option<Arc<NamespaceIndex>> {
        self.namespaces.read().await.get(ns).cloned()
    }

    pub async fn get_stream(&self, ns: &str, stream: &str) -> Option<Arc<StreamMeta>> {
        let ns = self.get_namespace(ns).await?;
        let meta_opt = ns.streams.read().await.get(stream).map(Arc::clone);
        meta_opt
    }

    pub async fn get_pipeline(&self, ns: &str, pipeline: &str) -> Option<Arc<PipelineMeta>> {
        let ns = self.get_namespace(ns).await?;
        let meta_opt = ns.pipelines.read().await.get(pipeline).map(Arc::clone);
        meta_opt
    }
}

/// An index structure used to track the resources of a namespace.
#[derive(Default)]
pub struct NamespaceIndex {
    /// A description of this namespace.
    pub description: String,
    /// Index of endpoints in this namespace.
    pub endpoints: RwLock<HashMap<String, ()>>,
    /// Index of streams in this namespace.
    pub streams: RwLock<HashMap<String, Arc<StreamMeta>>>,
    /// Index of pipelines in this namespace.
    pub pipelines: RwLock<HashMap<String, Arc<PipelineMeta>>>,
}

pub struct StreamMeta {
    /// The next available entry ID for this stream.
    pub next_id: RwLock<u64>,
    /// A description of this stream.
    pub description: String,
}

impl StreamMeta {
    pub fn new(description: String) -> Self {
        Self {
            next_id: Default::default(),
            description,
        }
    }
}

pub struct PipelineMeta {
    /// The next available ID for this pipeline.
    pub next_id: RwLock<u64>,
    /// The name of the stream which may trigger this pipeline.
    pub trigger: String,
    /// A description of this stream.
    pub description: String,
}

impl PipelineMeta {
    pub fn new(trigger: String, description: String) -> Self {
        Self {
            next_id: Default::default(),
            trigger,
            description,
        }
    }
}

/// A batch of operations to apply to the index atomically.
#[derive(Default)]
pub(super) struct IndexWriteBatch {
    ops: Vec<IndexWriteOp>,
}

impl IndexWriteBatch {
    pub(super) fn push(&mut self, op: IndexWriteOp) {
        self.ops.push(op);
    }
}

#[allow(clippy::enum_variant_names)]
pub(super) enum IndexWriteOp {
    /// Insert a new namespace record into the index.
    InsertNamespace { name: String, description: String },
    /// Insert a new stream record into the index.
    InsertStream { namespace: String, name: String, meta: StreamMeta },
    /// Insert a new pipeline record into the index.
    InsertPipeline { namespace: String, name: String, meta: PipelineMeta },
}
