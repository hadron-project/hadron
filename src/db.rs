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

use actix::prelude::*;
use log::{info};
use sled;
use uuid;

use crate::{
    App,
    common::NodeId,
    config::Config,
};

/// The default DB path to use for the data store.
const DEFAULT_DB_PATH: &str = "/var/lib/railgun/data";

/// The key used for storing the node ID of the current node.
const NODE_ID_KEY: &str = "id";

/// The database actor.
///
/// This actor is responsible for handling all runtime interfacing with the database.
pub struct Database {
    #[allow(dead_code)]
    app: Addr<App>,
    id: NodeId,
    #[allow(dead_code)]
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
        let id: String = match db.get(NODE_ID_KEY)? {
            Some(id) => String::from_utf8_lossy(&*id).to_string(),
            None => {
                let id = uuid::Uuid::new_v4().to_string();
                db.set(NODE_ID_KEY, id.as_bytes())?;
                id
            }
        };

        info!("Node ID is {}", &id);
        Ok(Database{app, id, db})
    }

    /// The defalt DB path to use.
    pub fn default_db_path() -> String {
        DEFAULT_DB_PATH.to_string()
    }

    /// This node's ID.
    pub fn node_id(&self) -> &NodeId {
        &self.id
    }
}
