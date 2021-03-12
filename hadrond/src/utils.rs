#![allow(dead_code)] // TODO: remove this.

use anyhow::{bail, Context, Result};
use prost::encoding::bool::encode;
use serde::{de::DeserializeOwned, Serialize};
use tokio::sync::oneshot::error::RecvError;
use tonic::Status;

use crate::error::AppError;

const ERR_BINCODE_ENCODE: &str = "error from bincode while serializing message to bytes";
const ERR_BINCODE_DECODE: &str = "error from bincode while deserializing message from bytes";

pub const ERR_DECODE_RAFT_RPC: &str = "error decoding Raft RPC";
pub const ERR_DECODE_RAFT_RPC_RESPONSE: &str = "error decoding Raft RPC response";
pub const ERR_ENCODE_RAFT_RPC: &str = "error encoding Raft RPC";

pub const HEADER_X_HADRON_AUTH: &str = "x-hadron-authorization";

pub const HIERARCHY_TOKEN: &str = ".";

/// A result type used for the gRPC layer.
pub type TonicResult<T> = std::result::Result<T, Status>;

/// Encode a stream entry key using the given prefix & offset.
pub fn encode_entry_key(prefix: &[u8; 6], offset: u64) -> [u8; 14] {
    let mut key = [0u8; 14];
    match prefix {
        [b0, b1, b2, b3, b4, b5] => {
            key[0] = *b0;
            key[1] = *b1;
            key[2] = *b2;
            key[3] = *b3;
            key[4] = *b4;
            key[5] = *b5;
        }
    }
    match encode_u64(offset) {
        [b6, b7, b8, b9, b10, b11, b12, b13] => {
            key[6] = b6;
            key[7] = b7;
            key[8] = b8;
            key[9] = b9;
            key[10] = b10;
            key[11] = b11;
            key[12] = b12;
            key[13] = b13;
        }
    }
    key
}

/// Encode the given u64 as an array of big-endian bytes.
pub fn encode_u64(val: u64) -> [u8; 8] {
    val.to_be_bytes()
}

/// Decode the given bytes as a u64.
pub fn decode_u64(val: &[u8]) -> Result<u64> {
    match val {
        [b0, b1, b2, b3, b4, b5, b6, b7] => Ok(u64::from_be_bytes([*b0, *b1, *b2, *b3, *b4, *b5, *b6, *b7])),
        _ => bail!("invalid byte array given to decode as u64, invalid len {} needed 8", val.len()),
    }
}

/// Encode the given i64 as an array of big-endian bytes.
pub fn encode_i64(val: i64) -> [u8; 8] {
    val.to_be_bytes()
}

/// Decode the given bytes as a i64.
pub fn decode_i64(val: &[u8]) -> Result<i64> {
    match val {
        [b0, b1, b2, b3, b4, b5, b6, b7] => Ok(i64::from_be_bytes([*b0, *b1, *b2, *b3, *b4, *b5, *b6, *b7])),
        _ => bail!("invalid byte array given to decode as i64, invalid len {} needed 8", val.len()),
    }
}

/// Encode the given object as Flexbuffers bytes.
pub fn encode_flexbuf<T: Serialize>(msg: &T) -> Result<Vec<u8>> {
    Ok(flexbuffers::to_vec(msg).context("error encoding object as flexbuf")?)
}

/// Decode the given Flexbuffers bytes as the target object.
pub fn decode_flexbuf<T: DeserializeOwned>(buf: &[u8]) -> Result<T> {
    Ok(flexbuffers::from_slice(buf).context("error decoding object as flexbuf")?)
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
