use std::hash::Hasher;

use anyhow::{bail, Context, Result};
use prost::Message;

use crate::error::AppError;

pub const HEADER_OCTET_STREAM: &str = "application/octet-stream";
pub const HEADER_APP_PROTO: &str = "application/protobuf";

pub const HIERARCHY_TOKEN: &str = ".";

/// Encode a stream entry key using the given prefix & offset.
pub fn encode_3_byte_prefix(prefix: &[u8; 3], offset: u64) -> [u8; 12] {
    let mut key = [0u8; 12];
    match prefix {
        [b0, b1, b2] => {
            key[0] = *b0;
            key[1] = *b1;
            key[2] = *b2;
        }
    }
    match encode_u64(offset) {
        [b3, b4, b5, b6, b7, b8, b9, b10] => {
            key[3] = b3;
            key[4] = b4;
            key[5] = b5;
            key[6] = b6;
            key[7] = b7;
            key[8] = b8;
            key[9] = b9;
            key[10] = b10;
        }
    }
    key[11] = b'/';
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

/// Encode the given model into a bytes vec.
pub fn encode_model<M: Message>(model: &M) -> Result<Vec<u8>> {
    let mut buf = Vec::with_capacity(model.encoded_len());
    model.encode(&mut buf).context("error serializing data model")?;
    Ok(buf)
}

/// Decode an object from the given buffer.
pub fn decode_model<M: Message + Default>(data: &[u8]) -> Result<M> {
    M::decode(data).context("error decoding object from storage")
}

/// Generate a hash code ID for the given namespace & name combination.
///
/// This generates a hash over `{ns}/{name}`.
pub fn ns_name_hash_id(ns: &str, name: &str) -> u64 {
    let mut hasher = seahash::SeaHasher::new();
    hasher.write(ns.as_bytes());
    hasher.write_u8(b'/');
    hasher.write(name.as_bytes());
    hasher.finish()
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
