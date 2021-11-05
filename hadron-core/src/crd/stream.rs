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
    printcolumn = r#"{"name":"Partitions","type":"number","jsonPath":".spec.partitions"}"#,
    printcolumn = r#"{"name":"Debug","type":"boolean","jsonPath":".spec.debug"}"#,
    printcolumn = r#"{"name":"Retention Policy","type":"string","jsonPath":".spec.retentionPolicy"}"#,
    printcolumn = r#"{"name":"PVC Volume Size","type":"string","jsonPath":".spec.pvcVolumeSize"}"#,
    printcolumn = r#"{"name":"PVC Access Modes","type":"string","jsonPath":".spec.pvcAccessModes"}"#,
    printcolumn = r#"{"name":"PVC Storage Class","type":"string","jsonPath":".spec.pvcStorageClass"}"#
)]
#[serde(rename_all = "camelCase")]
pub struct StreamSpec {
    /// The number of partitions to be created for this stream.
    ///
    /// This value can be dynamically scaled up and down and directly corresponds to the number of
    /// pods in the corresponding StatefulSet. Scaling down the number of partitions
    /// will cause the data of the removed partitions to be lost. Use with care.
    pub partitions: u32,
    /// Enable debug mode for the Stream's StatefulSet pods.
    #[serde(default)]
    pub debug: bool,
    /// The retention policy to use for data on the Stream.
    #[serde(default)]
    pub retention_policy: StreamRetentionSpec,

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

/// The data retention spec for the data on the Stream.
///
/// Defaults to `Time` based retention of 7 days.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StreamRetentionSpec {
    /// Retention policy to use.
    pub strategy: StreamRetentionPolicy,
    /// For `Time` retention policy, this specifies the amount of time
    /// to retain data on the Stream in seconds.
    #[serde(default)]
    pub retention_seconds: Option<u64>,
}

impl StreamRetentionSpec {
    /// The default retention seconds value, which is 7 days.
    pub fn retention_seconds_default() -> u64 {
        604_800 // 7 days.
    }
}

impl Default for StreamRetentionSpec {
    fn default() -> Self {
        Self {
            strategy: StreamRetentionPolicy::Time,
            retention_seconds: Some(StreamRetentionSpec::retention_seconds_default()),
        }
    }
}

/// The retention policy to use for data on the Stream.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum StreamRetentionPolicy {
    /// Retain data on the Stream indefinitely.
    Retain,
    /// Retain data on the Stream based on secondary timestamp index.
    Time,
}

impl std::fmt::Display for StreamRetentionPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Self::Retain => "retain",
                Self::Time => "time",
            }
        )
    }
}
