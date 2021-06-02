//! Stream CRD.
//!
//! The code here is used to generate the actual CRD used in K8s. See examples/crd.rs.

use kube::CustomResource;
use schemars::JsonSchema;
use serde::{de::Error as SerdeError, Deserialize, Deserializer, Serialize};

/// CRD spec for the Stream resource.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, CustomResource, JsonSchema)]
#[kube(
    struct = "Stream",
    status = "StreamStatus",
    group = "hadron.rs",
    version = "v1",
    kind = "Stream",
    namespaced,
    derive = "PartialEq",
    apiextensions = "v1",
    shortname = "stream",
    printcolumn = r#"{"name":"Partitions","type":"number","jsonPath":".spec.partitions"}"#,
    printcolumn = r#"{"name":"Replicas","type":"number","jsonPath":".spec.replicas"}"#,
    printcolumn = r#"{"name":"TTL","type":"number","jsonPath":".spec.ttl"}"#
)]
pub struct StreamSpec {
    /// The number of partitions to be created for this stream.
    pub partitions: u8,
    /// The number of replicas to be used per partition.
    pub replicas: u8,
    /// An optional TTL in seconds specifying how long records are to be kept on the stream.
    ///
    /// If `0`, then records will stay on the stream forever.
    pub ttl: u64,
}

/// CRD status object.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, JsonSchema)]
pub struct StreamStatus {
    /// The number of partitions to be created for this stream.
    pub partitions: u8,
    /// The number of replicas to be used per partition.
    pub replicas: u8,
}
