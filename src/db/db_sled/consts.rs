/// The path to the raft log.
pub(super) const RAFT_LOG_PREFIX: &str = "/raft/log/";
/// The key under which the Raft log's hard state is kept.
pub(super) const RAFT_HARDSTATE_KEY: &str = "/raft/hs";
/// The key under which the Raft last-applied-log index is kept.
pub(super) const RAFT_LAL_KEY: &str = "/raft/lal";
/// The key used for storing the node ID of the current node.
pub(super) const NODE_ID_KEY: &str = "id";

/// The DB path prefix for all users.
pub(super) const OBJECTS_USERS: &str = "/objects/users/";
/// The DB path prefix for all users.
pub(super) const OBJECTS_TOKENS: &str = "/objects/tokens/";
/// The DB path prefix for all namespaces.
pub(super) const OBJECTS_NS: &str = "/objects/ns/";
/// The DB path prefix for all namespaces.
pub(super) const OBJECTS_ENDPOINTS: &str = "/objects/endpoints/";
/// The DB path prefix for all streams.
pub(super) const OBJECTS_STREAMS: &str = "/objects/streams/";
/// The DB path prefix for all pipelines.
pub(super) const OBJECTS_PIPELINES: &str = "/objects/pipelines/";

/// The prefix under which all streams store their data.
///
/// Streams MUST index their data as `/streams/<namespace>/<stream_name>/<entry_index>`.
pub(super) const STREAMS_DATA_PREFIX: &str = "/streams";
/// The key used for tracking the next index for the next entry to be written to the respective stream.
pub(super) const STREAM_NEXT_INDEX_KEY: &str = "index";

/// An error from deserializing an entry.
pub(super) const ERR_DESERIALIZE_ENTRY: &str = "Failed to deserialize entry from log. Data is corrupt.";
/// An error from deserializing HardState record.
pub(super) const ERR_DESERIALIZE_HS: &str = "Failed to deserialize HardState from storage. Data is corrupt.";
/// An error from deserializing a User record.
pub(super) const ERR_DESERIALIZE_USER: &str = "Failed to deserialize User from storage. Data is corrupt.";
/// An error from deserializing a Pipeline record.
pub(super) const ERR_DESERIALIZE_PIPELINE: &str = "Failed to deserialize Pipeline from storage. Data is corrupt.";
/// An error from deserializing a Stream record.
pub(super) const ERR_DESERIALIZE_STREAM: &str = "Failed to deserialize Stream from storage. Data is corrupt.";
/// An error from serializing a Stream record.
pub(super) const ERR_SERIALIZE_STREAM: &str = "Failed to serialize Stream for storage.";
// /// An error from deserializing a Stream Entry record.
// const ERR_DESERIALIZE_STREAM_ENTRY: &str = "Failed to deserialize Stream Entry from storage. Data is corrupt.";
/// An error from serializing a hard state record.
pub(super) const ERR_SERIALIZE_HARD_STATE: &str = "Error while serializing Raft HardState object.";
/// An error from writing a hard state record to disk.
pub(super) const ERR_WRITING_HARD_STATE: &str = "Error while writing Raft HardState to disk.";
/// An error while sending messages between the async/sync DB actors.
pub(super) const ERR_DURING_DB_MSG: &str = "Actix MailboxError during messaging betweenn DB actors.";
/// An initialization error where an entry was expected, but none was found.
pub(super) const ERR_MISSING_ENTRY: &str = "Unable to access expected log entry.";
/// An error from an endpoint's index being malformed.
pub(super) const ERR_MALFORMED_ENDPOINT_INDEX: &str = "Malformed index for endpoint.";
