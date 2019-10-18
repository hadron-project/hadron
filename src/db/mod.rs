//! A module encapsulating all logic for interfacing with the data storage system.

mod db_sled;
mod models;

pub(crate) use models::{
    Pipeline,
};

/// The default path to use for data store.
pub const DEFAULT_DB_PATH: &str = "/var/lib/railgun/data";

/// The default path to use for data store.
pub const DEFAULT_SNAPSHOT_SUBDIR: &str = "/raft/snapshots";

/// The defalt DB path to use.
pub fn default_db_path() -> String {
    DEFAULT_DB_PATH.to_string()
}

// Perform a compile time test to ensure that only one storage engine is configured.
// TODO: expand this once we have more than one backend.
// #[cfg(any(
//     all(feature = "storage-sled", any(feature = "backend-other")),
// ))]
// compile_error!("Only one storage engine may be active.");

/// The configured storage backend.
pub type Storage = db_sled::SledStorage;
