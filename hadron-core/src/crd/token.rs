//! Token CRD.
//!
//! The code here is used to generate the actual CRD used in K8s. See examples/crd.rs.

use anyhow::{ensure, Result};
use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::error::AppError;

pub type Token = TokenCRD; // Mostly to resolve a Rust Analyzer issue.

/// CRD spec for the Token resource.
///
/// Tokens are used to establish authentication & authorization controls within a Hadron cluster.
///
/// When Hadron detects a new Token CR, it will generate a new Kubernetes secret bearing the same
/// name as the respective Token CR. The generated secret will contain a JWT signed by the Hadron
/// cluster's private key.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, CustomResource, JsonSchema)]
#[kube(
    struct = "TokenCRD",
    status = "TokenStatus",
    group = "hadron.rs",
    version = "v1beta1",
    kind = "Token",
    namespaced,
    derive = "PartialEq",
    apiextensions = "v1",
    shortname = "token",
    printcolumn = r#"{"name":"All","type":"string","jsonPath":".spec.all"}"#,
    printcolumn = r#"{"name":"Token ID","type":"string","jsonPath":".status.token_id"}"#
)]
#[serde(rename_all = "camelCase")]
pub struct TokenSpec {
    /// Grant full access to all resources of the cluster.
    ///
    /// If this value is true, then all other values are ignored.
    #[serde(default)]
    pub all: bool,
    /// Permissions granted on ephemeral messaging exchanges.
    #[serde(default)]
    pub exchanges: Option<PubSubAccess>,
    /// Permissions granted on RPC endpoints.
    #[serde(default)]
    pub endpoints: Option<PubSubAccess>,
    /// Permissions granted on streams.
    #[serde(default)]
    pub streams: Option<PubSubAccess>,
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
    /// The ID of the created token.
    pub token_id: String,
}

impl TokenCRD {
    /// Ensure these claims are sufficient for publishing to the given stream.
    pub fn check_stream_pub_auth(&self, stream: &str) -> Result<()> {
        if self.spec.all {
            return Ok(());
        }
        ensure!(
            self.spec.streams.as_ref().map(|streams| streams.r#pub.iter().by_ref().any(|name| name == stream)).unwrap_or(false),
            AppError::Unauthorized,
        );
        Ok(())
    }

    /// Ensure these claims are sufficient for subscribing to the given stream.
    pub fn check_stream_sub_auth(&self, stream: &str) -> Result<()> {
        if self.spec.all {
            return Ok(());
        }
        ensure!(
            self.spec.streams.as_ref().map(|streams| streams.sub.iter().by_ref().any(|name| name == stream)).unwrap_or(false),
            AppError::Unauthorized,
        );
        Ok(())
    }
}
