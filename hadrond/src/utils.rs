#![allow(dead_code)] // TODO: remove this.

use anyhow::{bail, Context, Result};
use serde::{de::DeserializeOwned, Serialize};
use tokio::sync::oneshot::error::RecvError;
use tonic::Status;

use crate::error::AppError;

const ERR_BINCODE_ENCODE: &str = "error from bincode while serializing message to bytes";
const ERR_BINCODE_DECODE: &str = "error from bincode while deserializing message from bytes";

pub const ERR_DECODE_RAFT_RPC: &str = "error decoding Raft RPC";
pub const ERR_ENCODE_RAFT_RPC: &str = "error encoding Raft RPC";

pub const HEADER_X_HADRON_AUTH: &str = "x-hadron-authorization";

pub const HIERARCHY_TOKEN: &str = ".";

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

/// Encode the given protobuf message as bytes.
pub fn encode_proto<T: prost::Message>(msg: T) -> Result<Vec<u8>> {
    let mut buf = vec![0u8; msg.encoded_len()];
    msg.encode(&mut buf)?;
    Ok(buf)
}

/// Decode a buffer of bytes as a protobuf message.
pub fn decode_proto<T: prost::Message + Default>(buf: &[u8]) -> Result<T> {
    Ok(prost::Message::decode(buf)?)
}

/// Create an internal gRPC error variant based on oneshot recv error.
pub fn status_from_rcv_error(_: RecvError) -> Status {
    Status::internal("target server dropped internal response channel")
}

/// Map a result's err variant into a gRPC status for the error.
pub fn map_result_to_status<T>(res: Result<T>) -> TonicResult<T> {
    res.map_err(status_from_err)
}

/// Map an error object into a gRPC status.
pub fn status_from_err(err: anyhow::Error) -> Status {
    if let Some(app_err) = err.downcast_ref::<AppError>() {
        return app_err.into();
    }
    Status::internal(err.to_string())
}

/// Validate the hierarchy structure of the given object name.
pub fn validate_name_hierarchy(name: &str) -> Result<()> {
    for seg in name.split(HIERARCHY_TOKEN) {
        if seg.is_empty() {
            bail!(AppError::InvalidInput(format!(
                "name hierarchy `{}` is invalid, segments delimited by `.` may not be empty",
                name,
            )));
        }
    }
    Ok(())
}
