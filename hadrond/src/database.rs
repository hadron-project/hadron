//! Database management.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use sled::{Config as SledConfig, Db, IVec};
use tokio::sync::RwLock;

use crate::config::Config;
use crate::error::{ShutdownError, ShutdownResult};

pub type Tree = sled::Tree;

/// Error message indicating an unsafe node name change was detected.
const NODE_NAME_MISMATCH_ERR: &str = r#"UNSAFE CONFIG CHANGE ERROR! It is not safe to change the node name \
of a node which already has state."#;

/// The default path to use for data storage.
pub const DEFAULT_DATA_PATH: &str = "/usr/local/hadron/data";
/// The dir used to back the database.
const DATABASE_DIR: &str = "db"; // <dataDir>/db
/// The name of the file used to hold the node's ID.
const NODE_ID_FILE_NAME: &str = "node_id"; // <dataDir>/node_id
/// The DB tree prefix used for streams.
const TREE_STREAM_PREFIX: &str = "streams";
/// The DB tree name of the metadata tree.
const TREE_METADATA: &str = "metadata";

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
    /// Open the database for usage.
    pub async fn new(config: Arc<Config>) -> Result<Self> {
        // Determine the database path, and ensure it exists.
        let dbpath = PathBuf::from(&config.storage_data_path).join(DATABASE_DIR);
        tokio::fs::create_dir_all(&dbpath)
            .await
            .context("error creating dir for hadron core database")?;

        // Ensure node ID file exists. If the node is not pristine,
        // then compare values & ensure match.
        let idfile = PathBuf::from(&config.storage_data_path).join(NODE_ID_FILE_NAME);
        match tokio::fs::read_to_string(&idfile).await {
            Ok(data) => {
                if data != config.node_name {
                    anyhow::bail!(NODE_NAME_MISMATCH_ERR);
                }
            }
            Err(err) => {
                if matches!(err.kind(), std::io::ErrorKind::NotFound) {
                    // Create the file.
                    tokio::fs::write(&idfile, config.node_name.as_bytes())
                        .await
                        .context("error writing contents of node ID file")?;
                } else {
                    Err(err).context("error reading contents of node ID file")?;
                }
            }
        }

        Self::spawn_blocking(move || -> Result<Self> {
            let db = SledConfig::new().path(dbpath).mode(sled::Mode::HighThroughput).open()?;
            let trees = RwLock::new(HashMap::new());
            let inner = Arc::new(DatabaseInner { config, db, trees });
            Ok(Self { inner })
        })
        .await?
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

    /// Generate a new ID.
    pub fn generate_id(&self) -> Result<u64> {
        self.inner.db.generate_id().context("error generating ID")
    }

    /// Get a handle to the DB tree for a stream partition replica.
    pub async fn get_metadata_tree(&self) -> ShutdownResult<Tree> {
        let (db, ivname) = (self.inner.db.clone(), IVec::from(TREE_METADATA));
        let tree = Self::spawn_blocking(move || -> Result<Tree> { Ok(db.open_tree(ivname)?) })
            .await
            .and_then(|res| res.map_err(|err| ShutdownError(anyhow!("could not open DB tree {} {}", TREE_METADATA, err))))?;
        Ok(tree)
    }

    /// Get a handle to the DB tree for a stream partition.
    pub async fn get_stream_tree(&self, namespace: &str, name: &str) -> ShutdownResult<Tree> {
        let name = format!(
            "{prefix}/{namespace}/{name}",
            prefix = TREE_STREAM_PREFIX,
            namespace = namespace,
            name = name
        );
        let (db, ivname) = (self.inner.db.clone(), IVec::from(name.as_str()));
        let tree = Self::spawn_blocking(move || -> Result<Tree> { Ok(db.open_tree(ivname)?) })
            .await
            .and_then(|res| res.map_err(|err| ShutdownError(anyhow!("could not open DB tree {} {}", &name, err))))?;
        Ok(tree)
    }

    /// Get a handle to the DB tree for a stream partition's metadata.
    pub async fn get_stream_tree_metadata(&self, namespace: &str, name: &str) -> ShutdownResult<Tree> {
        let name = format!(
            "{prefix}/{namespace}/{name}/metadata",
            prefix = TREE_STREAM_PREFIX,
            namespace = namespace,
            name = name
        );
        let (db, ivname) = (self.inner.db.clone(), IVec::from(name.as_str()));
        let tree = Self::spawn_blocking(move || -> Result<Tree> { Ok(db.open_tree(ivname)?) })
            .await
            .and_then(|res| res.map_err(|err| ShutdownError(anyhow!("could not open DB tree {} {}", &name, err))))?;
        Ok(tree)
    }
}
