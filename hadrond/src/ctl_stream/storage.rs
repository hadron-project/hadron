//! Raft storage backend for SPCs.

#![allow(unused_imports)] // TODO: remove this.
#![allow(unused_variables)] // TODO: remove this.
#![allow(unused_mut)] // TODO: remove this.
#![allow(dead_code)] // TODO: remove this.
#![allow(clippy::redundant_clone)] // TODO: remove this.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use async_raft::{async_trait::async_trait, AppData, AppDataResponse};
use sled::Tree;

use crate::config::Config;
use crate::database::Database;
use crate::proto::client::{StreamPubPayloadRequest, StreamPubPayloadResponse};
use crate::raft::{HadronRaft, RaftStorageSled, SledStorageProvider};
use crate::NodeId;

/// The Raft type used by SPCs.
pub(super) type SpcRaft = HadronRaft<StreamPubPayloadRequest, StreamPubPayloadResponse, SpcStorage>;

/// The storage provider used SPCs.
pub struct SpcStorage {
    /// The ID of this node in the cluster.
    id: NodeId,
    /// The application's runtime config.
    config: Arc<Config>,
    /// The directory where this Raft's snapshots are held.
    snapshot_dir: PathBuf,
    /// The database manager.
    db: Database,
    /// This controllers DB Tree used for its Raft log.
    log: Tree,
    /// This controllers DB Tree used for its Raft state machine.
    state: Tree,
}

impl SpcStorage {
    /// Create a new instance.
    pub fn new(id: NodeId, config: Arc<Config>, db: Database) -> Self {
        unimplemented!()
    }
}

#[async_trait]
impl SledStorageProvider for SpcStorage {
    /// The Raft request data type used by this implementation.
    type Req = StreamPubPayloadRequest;
    /// The Raft response data type used by this implementation.
    type Res = StreamPubPayloadResponse;

    /// Get the node ID of this Raft instance.
    fn get_id(&self) -> NodeId {
        unimplemented!()
    }
    /// Get a handle to the database Tree used for the Raft log.
    fn get_log(&self) -> Tree {
        unimplemented!()
    }
    /// Get a handle to the database Tree used for the Raft state machine.
    fn get_sm(&self) -> Tree {
        unimplemented!()
    }
    /// The path to this Raft's snapshot dir.
    fn snapshot_dir(&self) -> &Path {
        unimplemented!()
    }
    /// Recover system state when first initialized or after snapshot installation.
    ///
    /// This hook is provided as a way to perform initialization, build indices, or any other such
    /// procedures which might be needed when a new state is installed.
    async fn recover_system_state(&self) -> Result<()> {
        unimplemented!()
    }
    /// The internal business logic implementation of applying an entry to the state machine.
    async fn do_apply_entry_to_state_machine(&self, index: &u64, data: &Self::Req) -> Result<Self::Res> {
        unimplemented!()
    }
}

impl AppData for StreamPubPayloadRequest {}
impl AppDataResponse for StreamPubPayloadResponse {}
