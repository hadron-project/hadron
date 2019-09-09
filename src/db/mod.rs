//! A module encapsulating all logic for interfacing with the data storage system.

mod db_sled;

use actix_raft;

use crate::{
    proto::client::api::{
        AckPipelineRequest, AckStreamRequest,
        EnsurePipelineRequest, EnsureRpcEndpointRequest, EnsureStreamRequest,
        PubStreamRequest, SubPipelineRequest, SubStreamRequest,
        UnsubPipelineRequest, UnsubStreamRequest,
    },
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

/// All data variants which are persisted via Raft.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum AppData {
    PubStream(PubStreamRequest),
    SubStream(SubStreamRequest),
    SubPipeline(SubPipelineRequest),
    UnsubStream(UnsubStreamRequest),
    UnsubPipeline(UnsubPipelineRequest),
    EnsureRpcEndpoint(EnsureRpcEndpointRequest),
    EnsureStream(EnsureStreamRequest),
    EnsurePipeline(EnsurePipelineRequest),
    AckStream(AckStreamRequest),
    AckPipeline(AckPipelineRequest),
}

impl actix_raft::AppData for AppData {}
