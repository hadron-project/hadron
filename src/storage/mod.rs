//! The storage layer.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use anyhow::{Context, Result};
use async_std::path as async_path;

use crate::config::Config;

/// The default path to use for data storage.
pub const DEFAULT_DATA_PATH: &str = "/usr/local/hadron/data";

/// The name of the file used to hold the node's ID.
const NODE_ID_FILE_NAME: &str = "node_id";

/// The default path to use for data storage.
pub fn default_data_path() -> String {
    DEFAULT_DATA_PATH.to_string()
}

/// Fetch this node's ID from disk, or create a new ID if this node is pristine.
pub async fn get_node_id(config: &Config) -> Result<u64> {
    // Ensure the data dir exists.
    let data_dir_path = async_path::PathBuf::from(&config.storage_data_path);
    if !data_dir_path.exists().await {
        tokio::fs::create_dir_all(&data_dir_path)
            .await
            .context("error creating hadron data dir")?;
    }

    // If the node ID file does not exist, then roll a new ID and write it to disk.
    let node_id_file_path = data_dir_path.join(NODE_ID_FILE_NAME);
    if !node_id_file_path.exists().await {
        let mut hasher = DefaultHasher::default();
        uuid::Uuid::new_v4().hash(&mut hasher);
        let id: u64 = hasher.finish();
        tokio::fs::write(&node_id_file_path, format!("{}", id).as_bytes())
            .await
            .context("error writing node ID to disk")?;
        return Ok(id);
    }

    // If the node ID file does exist, then read its contents as u64.
    let node_id_raw = tokio::fs::read_to_string(&node_id_file_path)
        .await
        .context("error reading node ID file")?;
    let node_id = node_id_raw
        .parse::<u64>()
        .with_context(|| format!("invalid node ID found: {}", node_id_raw))?;
    Ok(node_id)
}

// Perform a compile time test to ensure that only one storage engine is configured.
// TODO: expand this once we have more than one backend.
// #[cfg(any(
//     all(feature = "storage-sled", any(feature = "backend-other")),
// ))]
// compile_error!("Only one storage engine may be active.");

// /// The configured storage backend.
// pub type Storage = db_sled::SledStorage;
