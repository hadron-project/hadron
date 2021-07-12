//! Database management.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use sled::{Config as SledConfig, Db, IVec};

use crate::config::Config;
use crate::error::{ShutdownError, ShutdownResult};

pub type Tree = sled::Tree;

/// The default path to use for data storage.
pub const DEFAULT_DATA_PATH: &str = "/usr/local/hadron/db";
/// The DB tree prefix used for the partition stream.
const TREE_STREAM: &str = "stream";
/// The DB tree prefix used for the partition stream's metadata.
const TREE_STREAM_METADATA: &str = "stream_metadata";
/// The DB tree prefix used for pipelines.
const TREE_PIPELINE_PREFIX: &str = "pipelines";
/// The DB tree prefix used for pipeline metadata.
const TREE_PIPELINE_METADATA: &str = "pipelines_metadata";

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
    #[allow(dead_code)]
    config: Arc<Config>,
    /// The underlying DB handle.
    db: Db,
}

impl Database {
    /// Open the database for usage.
    pub async fn new(config: Arc<Config>) -> Result<Self> {
        // Determine the database path, and ensure it exists.
        let dbpath = PathBuf::from(&config.storage_data_path).join(config.pod_name.as_str());
        tokio::fs::create_dir_all(&dbpath)
            .await
            .context("error creating dir for hadron core database")?;

        Self::spawn_blocking(move || -> Result<Self> {
            let db = SledConfig::new().path(dbpath).mode(sled::Mode::HighThroughput).open()?;
            let inner = Arc::new(DatabaseInner { config, db });
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

    /// Get a handle to the DB tree for a stream partition.
    pub async fn get_stream_tree(&self) -> ShutdownResult<Tree> {
        let (db, ivname) = (self.inner.db.clone(), IVec::from(TREE_STREAM));
        let tree = Self::spawn_blocking(move || -> Result<Tree> { Ok(db.open_tree(ivname)?) })
            .await
            .and_then(|res| res.map_err(|err| ShutdownError(anyhow!("could not open DB tree {} {}", TREE_STREAM, err))))?;
        Ok(tree)
    }

    /// Get a handle to the DB tree for a stream partition's metadata.
    pub async fn get_stream_tree_metadata(&self) -> ShutdownResult<Tree> {
        let (db, ivname) = (self.inner.db.clone(), IVec::from(TREE_STREAM_METADATA));
        let tree = Self::spawn_blocking(move || -> Result<Tree> { Ok(db.open_tree(ivname)?) })
            .await
            .and_then(|res| res.map_err(|err| ShutdownError(anyhow!("could not open DB tree {} {}", TREE_STREAM_METADATA, err))))?;
        Ok(tree)
    }

    /// Get a handle to the DB tree for a pipeline partition.
    pub async fn get_pipeline_tree(&self, name: &str) -> ShutdownResult<Tree> {
        let name = format!("{}/{}", TREE_PIPELINE_PREFIX, name);
        let (db, ivname) = (self.inner.db.clone(), IVec::from(name.as_str()));
        let tree = Self::spawn_blocking(move || -> Result<Tree> { Ok(db.open_tree(ivname)?) })
            .await
            .and_then(|res| res.map_err(|err| ShutdownError(anyhow!("could not open DB tree {} {}", &name, err))))?;
        Ok(tree)
    }

    /// Get a handle to the DB tree for a pipeline partition's metadata.
    pub async fn get_pipeline_tree_metadata(&self, name: &str) -> ShutdownResult<Tree> {
        let name = format!("{}/{}", TREE_PIPELINE_METADATA, name);
        let (db, ivname) = (self.inner.db.clone(), IVec::from(name.as_str()));
        let tree = Self::spawn_blocking(move || -> Result<Tree> { Ok(db.open_tree(ivname)?) })
            .await
            .and_then(|res| res.map_err(|err| ShutdownError(anyhow!("could not open DB tree {} {}", &name, err))))?;
        Ok(tree)
    }
}
