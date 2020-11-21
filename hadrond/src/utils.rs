use anyhow::{Context, Result};
use serde::{de::DeserializeOwned, Serialize};
use tokio::sync::oneshot::error::RecvError;
use tonic::Status;

use crate::auth::AuthError;

const ERR_BINCODE_ENCODE: &str = "error from bincode while serializing message to bytes";
const ERR_BINCODE_DECODE: &str = "error from bincode while deserializing message from bytes";
pub const ERR_ENCODE_RAFT_RPC: &str = "error encoding Raft RPC";
pub const ERR_DECODE_RAFT_RPC: &str = "error decoding Raft RPC";
pub const HEADER_X_HADRON_AUTH: &str = "x-hadron-authorization";

/// A result type used for the gRPC layer.
pub type TonicResult<T> = std::result::Result<T, Status>;

/// Encode the given payload using bincode.
pub fn bin_encode<T: Serialize>(payload: &T) -> Result<Vec<u8>> {
    bincode::serialize(payload).context(ERR_BINCODE_ENCODE)
}
/// Decode the given payload using bincode.
pub fn bin_decode<T: DeserializeOwned>(payload: &[u8]) -> Result<T> {
    bincode::deserialize(payload).context(ERR_BINCODE_DECODE)
}
/// Encode the given u64 as a bytes array.
///
/// This encodes the bytes in little-endian format. ANY CHANGE to this will cause data corruption.
pub fn encode_u64(val: u64) -> [u8; 8] {
    val.to_le_bytes()
}

/// Create an internal gRPC error variant based on oneshot recv error.
pub fn status_from_rcv_error(_: RecvError) -> Status {
    Status::internal("target server dropped internal response channel")
}

/// Create a gRPC result based on handling of various application level errors.
pub fn map_result_to_status<T>(res: Result<T>) -> TonicResult<T> {
    res.map_err(|err| match err.downcast_ref::<AuthError>() {
        Some(auth_err) => match auth_err {
            AuthError::Unauthorized => Status::permission_denied(auth_err.to_string()),
            AuthError::UnknownToken => Status::unauthenticated(auth_err.to_string()),
        },
        None => Status::internal(err.to_string()),
    })
}
