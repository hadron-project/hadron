#![allow(dead_code)]

use std::iter::FromIterator;

use anyhow::{bail, Context, Result};
use prost::Message;
use sled::IVec;

/// The CloudEvents spec version currently being used.
pub const CLOUD_EVENTS_SPEC_VERSION: &str = "1.0";

/// Encode a byte + u64 prefix key.
///
/// This allows for efficient BTree prefix storage without the overhead of allocating additional
/// vectors, strings or other sorts of buffers.
///
/// NOTE: if any data in a tree is encoded with a prefix, then all data in that stream will need
/// to be encoded with a well-defined prefix as well in order to avoid unintended collisions
/// and or data corruption.
pub fn encode_byte_prefix(prefix: &[u8; 1], offset: u64) -> [u8; 9] {
    let mut key = [0u8; 9];
    key[0] = prefix[0];
    match encode_u64(offset) {
        [b1, b2, b3, b4, b5, b6, b7, b8] => {
            key[1] = b1;
            key[2] = b2;
            key[3] = b3;
            key[4] = b4;
            key[5] = b5;
            key[6] = b6;
            key[7] = b7;
            key[8] = b8;
        }
    }
    key
}

/// Encode a byte + i64 prefix key.
///
/// See `encode_byte_prefix` for more details.
pub fn encode_byte_prefix_i64(prefix: &[u8; 1], offset: i64) -> [u8; 9] {
    let mut key = [0u8; 9];
    key[0] = prefix[0];
    match encode_i64(offset) {
        [b1, b2, b3, b4, b5, b6, b7, b8] => {
            key[1] = b1;
            key[2] = b2;
            key[3] = b3;
            key[4] = b4;
            key[5] = b5;
            key[6] = b6;
            key[7] = b7;
            key[8] = b8;
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

/// Encode the given bytes iterator as an IVec.
pub fn ivec_from_iter<T: IntoIterator<Item = u8>>(data: T) -> IVec {
    IVec::from_iter(data)
}
