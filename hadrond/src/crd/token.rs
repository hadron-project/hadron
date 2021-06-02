//! Token CRD.
//!
//! The code here is used to generate the actual CRD used in K8s. See examples/crd.rs.

use anyhow::{ensure, Result};
use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::AppError;

/// CRD spec for the Token resource.
///
/// Tokens are used to establish authentication & authorization controls within a Hadron cluster.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, CustomResource, JsonSchema)]
#[kube(
    struct = "Token",
    status = "TokenStatus",
    group = "hadron.rs",
    version = "v1",
    kind = "Token",
    namespaced,
    derive = "PartialEq",
    apiextensions = "v1",
    shortname = "token",
    printcolumn = r#"{"name":"Cluster","type":"string","jsonPath":".spec.cluster"}"#,
    printcolumn = r#"{"name":"Secret","type":"string","jsonPath":".status.secretName"}"#
)]
pub struct TokenSpec {
    /// The cluster to which this token belongs.
    pub cluster: String,
    /// Grant full access to all resources of the cluster.
    ///
    /// If this value is true, then all other values are ignored.
    pub all: bool,
    /// Permissions granted on ephemeral messaging exchanges.
    ///
    /// All grants are structured as `{name}={access}` and are comma-separated for multiple grants.
    pub exchanges: PubSubAccess,
    /// Permissions granted on RPC endpoints.
    ///
    /// All grants are structured as `{name}={access}` and are comma-separated for multiple grants.
    pub endpoints: PubSubAccess,
    /// Permissions granted on streams.
    ///
    /// All grants are structured as `{name}={access}` and are comma-separated for multiple grants.
    pub streams: PubSubAccess,
}

/// A pub/sub access spec.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, JsonSchema)]
pub struct PubSubAccess {
    /// Object names for which publishing access is granted.
    pub r#pub: Vec<String>,
    /// Object names for which subscription access is granted.
    pub sub: Vec<String>,
}

/// CRD status object.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TokenStatus {
    /// The name of the corresponding secret once created.
    pub secret_name: String,
    /// The ID of the created token.
    pub token_id: String,
}

impl Token {
    /// Ensure these claims are sufficient for publishing to the given stream.
    pub fn check_stream_pub_auth(&self, stream: &str) -> Result<()> {
        if self.spec.all {
            return Ok(());
        }
        ensure!(self.spec.streams.r#pub.iter().by_ref().any(|name| name == stream), AppError::Unauthorized);
        Ok(())
    }

    /// Ensure these claims are sufficient for subscribing to the given stream.
    pub fn check_stream_sub_auth(&self, stream: &str) -> Result<()> {
        if self.spec.all {
            return Ok(());
        }
        ensure!(self.spec.streams.sub.iter().by_ref().any(|name| name == stream), AppError::Unauthorized);
        Ok(())
    }
}
