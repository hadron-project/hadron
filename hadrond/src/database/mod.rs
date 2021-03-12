//! Database management.

use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use sled::{Config as SledConfig, Db, IVec};
use tokio::sync::RwLock;

use crate::config::Config;
use crate::error::{ShutdownError, ShutdownResult};

pub type Tree = sled::Tree;

/// The default path to use for data storage.
pub const DEFAULT_DATA_PATH: &str = "/usr/local/hadron/data";
/// The dir used to back the database.
const DATABASE_DIR: &str = "db"; // <dataDir>/db
/// The name of the file used to hold the node's ID.
const NODE_ID_FILE_NAME: &str = "node_id"; // <dataDir>/node_id

/// The DB tree of the CRC Raft log.
const TREE_CRC_LOG: &str = "crc::log";
/// The DB tree of the CRC Raft state machine.
const TREE_CRC_SM: &str = "crc::state_machine";
/// The DB tree prefix used for streams.
const TREE_STREAM_PREFIX: &str = "spc::stream";

/// The default path to use for data storage.
pub fn default_data_path() -> String {
    DEFAULT_DATA_PATH.to_string()
}

/// An abstraction over the Hadron database.
#[derive(Clone)]
pub struct Database {
    inner: Arc<DatabaseInner>,
}

struct DatabaseInner {
    /// System runtime config.
    config: Arc<Config>,
    /// The underlying DB handle.
    db: Db,
    /// A cache of DB trees which are never dropped.
    trees: RwLock<HashMap<IVec, Tree>>,
}

impl Database {
    /// Open the database for usage, caching all known tree names.
    pub async fn new(config: Arc<Config>) -> Result<Self> {
        // Determine the database path, and ensure it exists.
        let dbpath = PathBuf::from(&config.storage_data_path).join(DATABASE_DIR);
        tokio::fs::create_dir_all(&dbpath)
            .await
            .context("error creating dir for hadron core database")?;

        Self::spawn_blocking(move || -> Result<Self> {
            let db = SledConfig::new().path(dbpath).mode(sled::Mode::HighThroughput).open()?;
            let trees = RwLock::new(HashMap::new());
            let inner = Arc::new(DatabaseInner { config, db, trees });
            Ok(Self { inner })
        })
        .await?
    }

    /// Fetch this node's ID from disk, or create a new ID if this node is pristine.
    pub async fn get_node_id(&self) -> Result<u64> {
        // Read the contents of the ID file, creating a new one if needed.
        let node_id_file_path = std::path::PathBuf::from(&self.inner.config.storage_data_path).join(NODE_ID_FILE_NAME);
        let node_id_raw = match tokio::fs::read_to_string(&node_id_file_path).await {
            Ok(node_id_raw) => node_id_raw,
            Err(err) => match err.kind() {
                std::io::ErrorKind::NotFound => {
                    let mut hasher = DefaultHasher::default();
                    uuid::Uuid::new_v4().hash(&mut hasher);
                    let id: u64 = hasher.finish();
                    tokio::fs::write(&node_id_file_path, format!("{}", id).as_bytes())
                        .await
                        .context("error writing node ID to disk")?;
                    return Ok(id);
                }
                _ => return Err(err).context("error reading node ID file"),
            },
        };
        let node_id = node_id_raw
            .parse::<u64>()
            .with_context(|| format!("invalid node ID found: {}", node_id_raw))?;

        Ok(node_id)
    }

    /// Spawn a blocking database-related function, returning a ShutdownError if anything goes
    /// wrong related to spawning & joining.
    #[tracing::instrument(level = "trace", skip(f), err)]
    pub async fn spawn_blocking<F, R>(f: F) -> ShutdownResult<R>
    where
        F: FnOnce() -> R + Send + 'static,
        R: Send + 'static,
    {
        tokio::task::spawn_blocking(f)
            .await
            .map_err(|err| ShutdownError::from(anyhow::Error::from(err)))
    }

    /// Gets the target DB tree by name, creating it if it does not exist, and caching it before
    /// returning a handle to the new tree.
    async fn get_cached_tree(&self, name: &str) -> ShutdownResult<Tree> {
        // If we already have the tree in our mapping, clone it and return.
        if let Some(tree) = self.inner.trees.read().await.get(name.as_bytes()).cloned() {
            return Ok(tree);
        }

        // Else, we need to create the tree.
        let (db, ivname) = (self.inner.db.clone(), IVec::from(name));
        let tree = Self::spawn_blocking(move || -> Result<Tree> { Ok(db.open_tree(ivname)?) })
            .await
            .and_then(|res| res.map_err(|err| ShutdownError(anyhow!("could not open DB tree {}", err))))?;

        self.inner.trees.write().await.insert(name.into(), tree.clone());
        Ok(tree)
    }

    /// Generate a new ID.
    pub fn generate_id(&self) -> Result<u64> {
        self.inner.db.generate_id().context("error generating ID")
    }

    /// The DB tree of the CRC Raft log.
    pub async fn crc_log(&self) -> ShutdownResult<Tree> {
        self.get_cached_tree(TREE_CRC_LOG).await
    }

    /// The DB tree for the CRC Raft state machine.
    pub async fn crc_sm(&self) -> ShutdownResult<Tree> {
        self.get_cached_tree(TREE_CRC_SM).await
    }

    /// Get a handle to the DB tree for a stream partition replica.
    pub async fn get_stream_tree(&self, namespace: &str, name: &str, partition: u32, replica_id: u64) -> ShutdownResult<Tree> {
        let name = format!(
            "{prefix}/{namespace}/{name}/{partition}/{replica_id}",
            prefix = TREE_STREAM_PREFIX,
            namespace = namespace,
            name = name,
            partition = partition,
            replica_id = replica_id,
        );
        self.get_cached_tree(&name).await
    }
}
