//! Data index management.

use std::sync::Arc;

use dashmap::DashMap;

use crate::auth::Claims;
use crate::error::AppError;
use crate::models::auth::{User, UserRole};
use crate::models::prelude::*;
use crate::models::schema;

/// An always up-to-date cluster metadata cache.
///
/// It is important to note that we must reduce lock contention as much as possible. This can
/// easily be achieved by simply dropping the lock guard as soon as possible whenever a lock is
/// taken, especially given that all internal data is Arc'd and can be easily cloned, allowing the
/// lock to be dropped very quickly.
#[derive(Default)]
pub struct MetadataCache {
    /// Index of all managed schema branches which have been applied to the system.
    schema_branches: DashMap<String, i64>,

    /// Index of namespaces in the system by ID.
    namespaces: DashMap<u64, Arc<schema::Namespace>>,
    /// Index of namespace names to IDs.
    namespace_names: DashMap<String, u64>,

    /// Index of endpoints by ID.
    endpoints: DashMap<u64, ()>,
    /// Index of endpoint names (hashed namespace/name) to ID.
    endpoint_names: DashMap<u64, u64>,

    /// Index of streams by ID.
    streams: DashMap<u64, Arc<schema::Stream>>,
    /// Index of stream names (hashed namespace/name) to ID.
    stream_names: DashMap<u64, u64>,

    /// Index of pipelines by ID.
    pipelines: DashMap<u64, Arc<schema::Pipeline>>,
    /// Index of pipeline names (hashed namespace/name) to ID.
    pipeline_names: DashMap<u64, u64>,

    /// Index of users.
    users: DashMap<String, Arc<User>>,
    /// Index of tokens.
    tokens: DashMap<u128, Arc<Claims>>,
}

impl MetadataCache {
    /// Create a new instance.
    pub fn new(
        users: Vec<User>, tokens: Vec<(u128, Claims)>, namespaces: Vec<Arc<schema::Namespace>>, streams: Vec<Arc<schema::Stream>>,
        pipelines: Vec<Arc<schema::Pipeline>>, schema_branches: Vec<schema::SchemaBranch>,
    ) -> Self {
        let index = Self::default();
        for val in schema_branches {
            let _ = index.schema_branches.insert(val.name, val.timestamp);
        }
        for val in users {
            let _ = index.users.insert(val.name.clone(), Arc::new(val));
        }
        for (key, val) in tokens {
            let _ = index.tokens.insert(key, Arc::new(val));
        }
        for val in namespaces {
            index.namespace_names.insert(val.name.clone(), val.id);
            index.namespaces.insert(val.id, val);
        }
        for val in streams {
            let hash = seahash::hash(val.namespaced_name().as_bytes());
            index.stream_names.insert(hash, val.id);
            index.streams.insert(val.id, val);
        }
        for val in pipelines {
            let hash = seahash::hash(val.namespaced_name().as_bytes());
            index.pipeline_names.insert(hash, val.id);
            index.pipelines.insert(val.id, val);
        }

        index
    }

    pub(super) fn apply_batch(&self, ops: CacheWriteBatch) {
        for op in ops.ops {
            match op {
                CacheWriteOp::InsertNamespace(ns) => {
                    self.namespace_names.insert(ns.name.clone(), ns.id);
                    self.namespaces.insert(ns.id, ns);
                }
                CacheWriteOp::InsertStream(stream) => {
                    let hash = seahash::hash(stream.namespaced_name().as_bytes());
                    self.stream_names.insert(hash, stream.id);
                    self.streams.insert(stream.id, stream);
                }
                CacheWriteOp::InsertPipeline(pipeline) => {
                    let hash = seahash::hash(pipeline.namespaced_name().as_bytes());
                    self.pipeline_names.insert(hash, pipeline.id);
                    self.pipelines.insert(pipeline.id, pipeline);
                }
                CacheWriteOp::UpdateSchemaBranch { branch, timestamp } => {
                    self.schema_branches.insert(branch, timestamp);
                }
            }
        }
    }

    /// Get a schema branch by its branch name.
    ///
    /// This returns the last applied timestamp of the schema branch.
    pub fn get_schema_branch(&self, branch: &str) -> Option<i64> {
        self.schema_branches.get(branch).map(|val| *val.value())
    }

    /// Get a namespace by its name.
    pub fn get_namespace(&self, ns: &str) -> Option<Arc<schema::Namespace>> {
        match self.namespace_names.get(ns).map(|val| *val.value()) {
            Some(id) => self.namespaces.get(&id).map(|val| val.value().clone()),
            None => None,
        }
    }

    /// Get a pipeline by its namespace and name.
    pub fn get_pipeline(&self, ns: &str, pipeline: &str) -> Option<Arc<schema::Pipeline>> {
        let hash = seahash::hash(format!("{}/{}", ns, pipeline).as_bytes());
        match self.pipeline_names.get(&hash).map(|val| *val.value()) {
            Some(id) => self.pipelines.get(&id).map(|val| val.value().clone()),
            None => None,
        }
    }

    /// Get a stream by its namespace & name.
    pub fn get_stream(&self, ns: &str, stream: &str) -> Option<Arc<schema::Stream>> {
        let hash = seahash::hash(format!("{}/{}", ns, stream).as_bytes());
        match self.stream_names.get(&hash).map(|val| *val.value()) {
            Some(id) => self.streams.get(&id).map(|val| val.value().clone()),
            None => None,
        }
    }

    /// Get the given token's claims, else return an auth error.
    pub fn must_get_token_claims(&self, token_id: &u128) -> anyhow::Result<Arc<Claims>> {
        match self.tokens.get(token_id).map(|val| val.value().clone()) {
            Some(claims) => Ok(claims),
            None => Err(AppError::UnknownToken.into()),
        }
    }

    /// Get the given user after validating the given password, else return an auth error.
    pub fn must_get_user(&self, user: &str, pass: &str) -> anyhow::Result<Arc<User>> {
        let user = match self.users.get(user) {
            Some(user) => user.value().clone(),
            None => return Err(AppError::UnknownUser.into()),
        };
        bcrypt::verify(pass, &user.pwhash).map_err(|_| AppError::InvalidCredentials("invalid username password combination".into()))?;
        Ok(user)
    }
}

/// A batch of operations to apply to the cache atomically.
#[derive(Default)]
pub(super) struct CacheWriteBatch {
    ops: Vec<CacheWriteOp>,
}

impl CacheWriteBatch {
    /// Push a new operation into this write batch.
    pub(super) fn push(&mut self, op: CacheWriteOp) {
        self.ops.push(op);
    }
}

#[allow(clippy::enum_variant_names)]
pub(super) enum CacheWriteOp {
    /// Insert a new namespace record into the index.
    InsertNamespace(Arc<schema::Namespace>),
    /// Insert a new stream record into the index.
    InsertStream(Arc<schema::Stream>),
    /// Insert a new pipeline record into the index.
    InsertPipeline(Arc<schema::Pipeline>),
    /// Update a schema branch with a new timestamp.
    UpdateSchemaBranch { branch: String, timestamp: i64 },
}
