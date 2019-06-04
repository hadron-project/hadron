//! A module encapsulating all logic for interfacing with the data storage system.
//!
//! The primary actor of this module is the `Database` actor. It handles various types of messages
//! corresponding to the various events which should cause data to be written to or read from the
//! database.
//!
//! There are two primary pathways into interfacing with the database:
//!
//! - Client events. A client request to begin reading a stream or a request to write data to a
//! stream. This always pertains to persistent streams. Ephemeral messaging does not touch the
//! database.
//! - Cluster consensus. The `Consensus` actor within the system will inevitibly write data to the
//! database. This data is treated much the same way that a persistent stream is treated. Every
//! record gets a monotonically increasing `u64` ID.
//!
//! It is important to note that, at this point, Railgun does not maintain a WAL of all data write
//! operations for persistent streams. This would be completely redundant and there is only one
//! type of operation supported on a stream: write the blob of data in the payload. So if a node
//! is behind and needs to catch up, reconstructing the events is as simple as reading the latest
//! events and writing them to the data store for the target stream.

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    sync::Arc,
};

use actix::prelude::*;
use log::{error, info};
use sled;
use uuid;

use crate::{
    App,
    common::NodeId,
    config::Config,
    proto::storage::raft::ClusterState,
};

/// The default DB path to use for the data store.
const DEFAULT_DB_PATH: &str = "/var/lib/railgun/data";

/// The database path to the rust log.
const RAFT_LOG_PATH: &str = "/cluster/raft/data/";

/// The key under which the Raft log's metadata is kept.
const RAFT_METADATA_KEY: &str = "/cluster/raft/metadata";

/// The key used for storing the node ID of the current node.
const NODE_ID_KEY: &str = "id";

/// The database actor.
///
/// This actor is responsible for handling all runtime interfacing with the database.
pub struct Database {
    _app: Addr<App>,
    id: NodeId,
    db: sled::Db,
}

impl Actor for Database {
    type Context = Context<Self>;
}

impl Database {
    /// Initialize the system database.
    ///
    /// This will initialize the data store, and will ensure that the database has a node ID.
    pub fn new(app: Addr<App>, config: &Config) -> Result<Self, sled::Error> {
        info!("Initializing database.");
        let db = sled::Db::start_default(&config.db_path)?;
        let mut hasher = DefaultHasher::default();
        let id: u64 = match db.get(NODE_ID_KEY)? {
            Some(id) => {
                hasher.write(id.as_ref());
                hasher.finish()
            },
            None => {
                uuid::Uuid::new_v4().hash(&mut hasher);
                let id: u64 = hasher.finish();
                db.set(NODE_ID_KEY, id.to_le_bytes().as_ref())?;
                id
            }
        };

        info!("Node ID is {}", &id);
        Ok(Database{_app: app, id, db})
    }

    /// The defalt DB path to use.
    pub fn default_db_path() -> String {
        DEFAULT_DB_PATH.to_string()
    }

    /// This node's ID.
    pub fn node_id(&self) -> NodeId {
        self.id
    }

    /// Create a Raft data storage instance for use by the consensus actor.
    pub fn raft_storage(&self) -> sled::Result<RaftStorage> {
        let log = self.db.open_tree(RAFT_LOG_PATH)?;
        Ok(RaftStorage{log, db: self.db.clone()})
    }
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// RaftStorage ///////////////////////////////////////////////////////////////////////////////////

/// A type used to implement the Raft storage interface.
pub struct RaftStorage {
    log: Arc<sled::Tree>,
    db: sled::Db,
}

impl RaftStorage {
    /// Get the cluster state from disk, else default.
    pub fn get_cluster_state(&self) -> sled::Result<ClusterState> {
        let state = self.db.get(RAFT_METADATA_KEY)?
            .map(|data| {
                use prost::Message;
                ClusterState::decode(&*data).unwrap_or_else(|err| {
                    error!("Failed to read cluster state from disk. Data may be corrupt. {}", err);
                    ClusterState::default()
                })
            }).unwrap_or_default();

        Ok(state)
    }
}

// impl raft::Storage for RaftStorage {
//     /// `initial_state` is called when Raft is initialized.
//     ///
//     /// This interface will return a RaftState which contains `HardState` and `ConfState`.
//     fn initial_state(&self) -> raft::Result<raft::RaftState> {
//         Ok(raft::RaftState::)
//     }

//     /// Returns a slice of log entries in the range `[low, high)`.
//     ///
//     /// `max_size` limits the total size of the log entries returned, but entries returns at
//     /// least one entry if any.
//     fn entries(&self, low: u64, high: u64, max_size: u64) -> raft::Result<Vec<raft::eraftpb::Entry>> {
//         Ok(Vec::with_capacity(0))
//     }

//     /// Returns the term of entry idx, which must be in the range `[first_index()-1, last_index()]`.
//     ///
//     /// The term of the entry before `first_index` is retained for matching purpose even though
//     /// the rest of that entry may not be available.
//     fn term(&self, idx: u64) -> raft::Result<u64> {
//         Ok(())
//     }

//     /// Returns the index of the first log entry that is possible available via entries (older
//     /// entries have been incorporated into the latest snapshot; if storage only contains the
//     /// dummy entry the first log entry is not available).
//     fn first_index(&self) -> raft::Result<u64> {
//         Ok(())
//     }

//     /// The index of the last entry in the log.
//     fn last_index(&self) -> raft::Result<u64> {
//         Ok(())
//     }

//     /// Returns the most recent snapshot.
//     ///
//     /// If snapshot is temporarily unavailable, it should return `SnapshotTemporarilyUnavailable`,
//     /// so raft state machine could know that Storage needs some time to prepare snapshot and
//     /// call snapshot later.
//     fn snapshot(&self) -> raft::Result<raft::eraftpb::Snapshot> {
//         Ok(())
//     }
// }
