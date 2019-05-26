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
    app: Addr<App>,
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
