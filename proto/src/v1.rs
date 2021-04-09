//! Client V1 protocol code.

use anyhow::{Context, Result};
use bincode::Options;
use bytes::{Buf, BufMut, Bytes, BytesMut};
use lazy_static::lazy_static;
use serde::{de::DeserializeOwned, Deserialize, Serialize};

lazy_static! {
    pub static ref CFG: bincode::config::DefaultOptions = bincode::config::DefaultOptions::new();
}

/// Write the given message into a new vector of bytes.
pub fn write<M: Serialize>(msg: &M) -> Result<Vec<u8>> {
    CFG.serialize(msg).context("error serializing message")
}

/// Write the given message into the given bytes buffer.
pub fn write_to_bytes<M: Serialize>(msg: &M, buf: &mut BytesMut) -> Result<()> {
    CFG.serialize_into(buf.writer(), msg).context("error serializing message to bytes")
}

/// Read the expected message type from the given bytes buffer.
pub fn read_from_bytes<M: DeserializeOwned>(buf: Bytes) -> Result<M> {
    CFG.deserialize_from(buf.reader()).context("error deserializing message")
}

///////////////////////////////////////////////////////////////////////////////
// Components /////////////////////////////////////////////////////////////////
//
// These types are used as composites in other types or in a standalone fashion.

/// An error object which is returned from the Hadron server under various conditions.
///
/// Clients can match on specific error variants to drive behavior.
#[derive(Clone, Debug, Serialize, Deserialize, thiserror::Error)]
#[error("{message}")]
pub struct Error {
    /// A summary of the error which has taken place.
    pub message: String,
}

/// Details on a Hadron replica set.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReplicaSet {
    /// The name of the replica set.
    ///
    /// This is immutable and is always used to identify partition assignment.
    pub name: String,
}

///////////////////////////////////////////////////////////////////////////////
// Metadata ///////////////////////////////////////////////////////////////////

/// The URL of the metadata endpoint.
pub const URL_METADATA: &str = "/metadata";

/// All known Hadron metadata.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MetadataResponse {
    // The name of the cluster which was queried.
    pub cluster_name: String,
    // Details on the replica set which was queried.
    pub replica_set: String,
    // All known replica sets in the cluster.
    pub all_replica_sets: Vec<ReplicaSet>,
}

///////////////////////////////////////////////////////////////////////////////
// Schema /////////////////////////////////////////////////////////////////////

/// The URL of the schema endpoint.
pub const URL_SCHEMA: &str = "/schema";

/// A request to update the schema of the Hadron cluster.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SchemaUpdateRequest {
    /// A managed schema update request.
    Managed(SchemaUpdateManaged),
    /// A one-off schema update request.
    OneOff(SchemaUpdateOneOff),
}

/// A response from an earlier `SchemaUpdateRequest`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SchemaUpdateResponse {
    /// A bool indicating if the request was a no-op, which would only apply to
    /// managed schema updates.
    pub was_noop: bool,
}

/// A managed schema update request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SchemaUpdateManaged {
    /// A set of Hadron schema documents to apply to the system.
    pub schema: String,
    /// The branch name of this set of schema updates.
    pub branch: String,
    /// The timestamp of this set of schema updates.
    ///
    /// This should be an epoch timestamp with millisecond precision.
    pub timestamp: i64,
}

/// A one-off schema update request.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SchemaUpdateOneOff {
    /// A set of Hadron schema documents to apply to the system.
    pub schema: String,
}
