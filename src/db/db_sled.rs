//! A module encapsulating all logic for interfacing with the data storage system.

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    sync::Arc,
};

use actix::prelude::*;
use actix_raft::{
    RaftStorage,
    messages::Entry,
    storage::{
        AppendLogEntry,
        ApplyToStateMachine,
        CreateSnapshot,
        CurrentSnapshotData,
        GetCurrentSnapshot,
        GetInitialState,
        GetLogEntries,
        HardState,
        InitialState,
        InstallSnapshot,
        InstallSnapshotChunk,
        ReplicateLogEntries,
        SaveHardState,
    },
};
use log::{info};
use sled;
use uuid;

use crate::{
    App, NodeId,
    config::Config,
    db::{AppData},
    proto::client::api::{ClientError, ErrorCode},
};

/// The path to the raft log.
pub(self) const RAFT_LOG_PREFIX: &str = "/raft/log/";

/// The key under which the Raft log's hard state is kept.
pub(self) const RAFT_HARDSTATE_KEY: &str = "/raft/hs";

/// The key used for storing the node ID of the current node.
pub(self) const NODE_ID_KEY: &str = "id";

/// The DB path prefix for all streams.
pub(self) const STREAMS_PATH_PREFIX: &str = "/streams/";

type RgEntry = Entry<AppData>;
type RgAppendLogEntry = AppendLogEntry<AppData, ClientError>;
type RgApplyToStateMachine = ApplyToStateMachine<AppData, ClientError>;
type RgCreateSnapshot = CreateSnapshot<ClientError>;
type RgGetCurrentSnapshot = GetCurrentSnapshot<ClientError>;
type RgGetInitialState = GetInitialState<ClientError>;
type RgGetLogEntries = GetLogEntries<AppData, ClientError>;
type RgInstallSnapshot = InstallSnapshot<ClientError>;
type RgReplicateLogEntries = ReplicateLogEntries<AppData, ClientError>;
type RgSaveHardState = SaveHardState<ClientError>;

//////////////////////////////////////////////////////////////////////////////////////////////////
// SledStorage ///////////////////////////////////////////////////////////////////////////////////

/// An implementation of the Railgun storage engine using Sled.
#[derive(Clone)]
pub struct SledStorage(Arc<SledStorageInner>);

struct SledStorageInner {
    _app: Addr<App>,
    id: NodeId,
    db: sled::Db,
    /// The Raft log.
    log: Arc<sled::Tree>,
}

impl SledStorage {
    /// Create a new instance.
    ///
    /// This will initialize the data store, and will ensure that the database has a node ID.
    pub fn new(app: Addr<App>, config: &Config) -> Result<Self, sled::Error> {
        info!("Initializing database.");
        let db = sled::Db::open(&config.db_path)?;
        let mut hasher = DefaultHasher::default();
        let id: u64 = match db.get(NODE_ID_KEY)? {
            Some(id) => {
                hasher.write(id.as_ref());
                hasher.finish()
            },
            None => {
                uuid::Uuid::new_v4().hash(&mut hasher);
                let id: u64 = hasher.finish();
                db.insert(NODE_ID_KEY, id.to_le_bytes().as_ref())?;
                id
            }
        };
        let log = db.open_tree(RAFT_LOG_PREFIX)?;

        info!("Node ID is {}", &id);
        Ok(SledStorage(Arc::new(SledStorageInner{_app: app, id, db, log})))
    }

    /// This node's ID.
    pub fn node_id(&self) -> NodeId {
        self.0.id
    }
}

impl Actor for SledStorage {
    type Context = SyncContext<Self>;
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// Impl RaftStorage //////////////////////////////////////////////////////////////////////////////

impl RaftStorage<AppData, ClientError> for SledStorage {
    type Actor = Self;
    type Context = SyncContext<Self>;
}

impl Handler<RgAppendLogEntry> for SledStorage {
    type Result = Result<(), ClientError>;

    fn handle(&mut self, _msg: RgAppendLogEntry, _ctx: &mut Self::Context) -> Self::Result {
        Ok(())
    }
}

impl Handler<RgApplyToStateMachine> for SledStorage {
    type Result = Result<(), ClientError>;

    fn handle(&mut self, _msg: RgApplyToStateMachine, _ctx: &mut Self::Context) -> Self::Result {
        Ok(())
    }
}

impl Handler<RgCreateSnapshot> for SledStorage {
    type Result = Result<CurrentSnapshotData, ClientError>;

    fn handle(&mut self, _msg: RgCreateSnapshot, _ctx: &mut Self::Context) -> Self::Result {
        Err(ClientError{code: ErrorCode::Internal as i32, message: String::from("")})
    }
}

impl Handler<RgGetCurrentSnapshot> for SledStorage {
    type Result = Result<Option<CurrentSnapshotData>, ClientError>;

    fn handle(&mut self, _msg: RgGetCurrentSnapshot, _ctx: &mut Self::Context) -> Self::Result {
        Ok(None)
    }
}

impl Handler<RgGetInitialState> for SledStorage {
    type Result = Result<InitialState, ClientError>;

    fn handle(&mut self, _msg: RgGetInitialState, _ctx: &mut Self::Context) -> Self::Result {
        Err(ClientError{code: ErrorCode::Internal as i32, message: String::from("")})
    }
}

impl Handler<RgGetLogEntries> for SledStorage {
    type Result = Result<Vec<RgEntry>, ClientError>;

    fn handle(&mut self, _msg: RgGetLogEntries, _ctx: &mut Self::Context) -> Self::Result {
        Ok(vec![])
    }
}

impl Handler<RgInstallSnapshot> for SledStorage {
    type Result = Result<(), ClientError>;

    fn handle(&mut self, _msg: RgInstallSnapshot, _ctx: &mut Self::Context) -> Self::Result {
        Ok(())
    }
}

impl Handler<RgReplicateLogEntries> for SledStorage {
    type Result = Result<(), ClientError>;

    fn handle(&mut self, _msg: RgReplicateLogEntries, _ctx: &mut Self::Context) -> Self::Result {
        Ok(())
    }
}

impl Handler<RgSaveHardState> for SledStorage {
    type Result = Result<(), ClientError>;

    fn handle(&mut self, _msg: RgSaveHardState, _ctx: &mut Self::Context) -> Self::Result {
        Ok(())
    }
}
