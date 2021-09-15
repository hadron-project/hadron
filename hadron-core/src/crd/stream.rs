//! Stream CRD.
//!
//! The code here is used to generate the actual CRD used in K8s. See examples/crd.rs.

use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub type Stream = StreamCRD; // Mostly to resolve a Rust Analyzer issue.

/// CRD spec for the Stream resource.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, CustomResource, JsonSchema)]
#[kube(
    struct = "StreamCRD",
    status = "StreamStatus",
    group = "hadron.rs",
    version = "v1beta1",
    kind = "Stream",
    namespaced,
    derive = "PartialEq",
    apiextensions = "v1",
    shortname = "stream",
    printcolumn = r#"{"name":"Cluster Name","type":"string","jsonPath":".spec.cluster_name"}"#,
    printcolumn = r#"{"name":"Partitions","type":"number","jsonPath":".spec.partitions"}"#,
    printcolumn = r#"{"name":"TTL","type":"number","jsonPath":".spec.ttl"}"#,
    printcolumn = r#"{"name":"Debug","type":"boolean","jsonPath":".spec.debug"}"#,
    printcolumn = r#"{"name":"PVC Volume Size","type":"string","jsonPath":".spec.pvc_volume_size"}"#,
    printcolumn = r#"{"name":"PVC Access Modes","type":"string","jsonPath":".spec.pvc_access_modes"}"#,
    printcolumn = r#"{"name":"PVC Storage Class","type":"string","jsonPath":".spec.pvc_storage_class"}"#
)]
pub struct StreamSpec {
    /// The CloudEvents root `source` of all events of this cluster.
    ///
    /// This value is used as the prefix of the `source` field of all events published to
    /// this stream, formatted as `{cluster_name}/{stream}/{partition}`.
    ///
    /// This value can be re-used for any number of streams.
    pub cluster_name: String,
    /// The number of partitions to be created for this stream.
    ///
    /// This value can be dynamically scaled up and down and directly corresponds to the number of
    /// pods in the corresponding StatefulSet. Scaling down the number of partitions
    /// will cause the data of the removed partitions to be lost. Use with care.
    pub partitions: u32,
    /// An optional TTL in seconds specifying how long records are to be kept on the stream.
    ///
    /// If `0`, then records will stay on the stream forever. Modifying this value will require a
    /// restart of the stream's StatefulSet.
    #[serde(default)]
    pub ttl: u64,
    /// Enable debug mode for the Stream's StatefulSet pods.
    #[serde(default)]
    pub debug: bool,

    /// Force an exact image to be used for the backing StatefulSet.
    ///
    /// Normally this will should not be set, and the Operator will ensure that the most recent
    /// semver compatible image is being used.
    pub image: String,

    /// The volume size to use for the Stream's backing StatefulSet PVCs.
    pub pvc_volume_size: String,
    /// The access modes to use for the Stream's backing StatefulSet PVCs.
    #[serde(default)]
    pub pvc_access_modes: Option<Vec<String>>,
    /// The storage class to use for the Stream's backing StatefulSet PVCs.
    #[serde(default)]
    pub pvc_storage_class: Option<String>,
}

/// CRD status object.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, JsonSchema)]
pub struct StreamStatus {}
