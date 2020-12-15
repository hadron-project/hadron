//! The storage layer.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use anyhow::{Context, Result};

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
    tokio::fs::create_dir_all(&config.storage_data_path)
        .await
        .context("error creating hadron data dir")?;

    // Read the contents of the ID file, creating a new one if needed.
    let node_id_file_path = std::path::PathBuf::from(&config.storage_data_path).join(NODE_ID_FILE_NAME);
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

// Perform a compile time test to ensure that only one storage engine is configured.
// TODO: expand this once we have more than one backend.
// #[cfg(any(
//     all(feature = "storage-sled", any(feature = "backend-other")),
// ))]
// compile_error!("Only one storage engine may be active.");

// /// The configured storage backend.
// pub type Storage = db_sled::SledStorage;
