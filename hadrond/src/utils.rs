use anyhow::{bail, Context, Result};
use serde::{de::DeserializeOwned, Serialize};

use crate::error::AppError;

pub const ERR_DECODE_RAFT_RPC: &str = "error decoding Raft RPC";
pub const ERR_DECODE_RAFT_RPC_RESPONSE: &str = "error decoding Raft RPC response";
pub const ERR_ENCODE_RAFT_RPC: &str = "error encoding Raft RPC";

pub const HEADER_X_HADRON_AUTH: &str = "x-hadron-authorization";
pub const HIERARCHY_TOKEN: &str = ".";

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
