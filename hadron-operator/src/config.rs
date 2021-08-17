//! Runtime configuration.

// TODO: finish up TLS config.

use anyhow::{Context, Result};
use jsonwebtoken::{DecodingKey, EncodingKey};
use serde::de::Error as DeError;
use serde::{Deserialize, Deserializer};

/// Runtime configuration data.
#[derive(Clone, Debug, Deserialize)]
pub struct Config {
    /// The server's logging config, which uses Rust's `env_logger` directives.
    pub rust_log: String,
    /// The port used for client interaction.
    pub client_port: u16,
    /// The port used for HTTP webhooks and healthchecks.
    pub http_port: u16,

    /// The Kubernetes namespace of this cluster.
    pub namespace: String,
    /// The name of the pod on which this instance is running.
    pub pod_name: String,

    /// The duration in seconds for which a lease is considered held.
    ///
    /// To ensure stable cluster leadership, a 60 second lease is currently recommended.
    pub lease_duration_seconds: u32,
    /// The duration that a lease holder will retry refreshing lease.
    ///
    /// To ensure stable cluster leadership, a 10 seconds renew rate is currently recommended.
    pub lease_renew_seconds: u32,

    /// The JWT encoding key.
    #[serde(deserialize_with = "Config::parse_encoding_key")]
    pub jwt_encoding_key: EncodingKey,
    /// The JWT decoding key, along with its original base64 encoded form.
    #[serde(deserialize_with = "Config::parse_decoding_key")]
    pub jwt_decoding_key: (DecodingKey<'static>, String),
}

// /// Cluster TLS mode.
// #[derive(Clone, Debug, Deserialize)]
// #[serde(rename_all = "UPPERCASE")]
// pub enum TlsMode {
//     /// TLS is disabled throughout the cluster.
//     None,
//     /// TLS is required for all connections throughout the cluster.
//     Required,
// }

impl Config {
    /// Create a new config instance.
    ///
    /// Currently this routing just parses the runtime environment and builds the application
    /// config from that. In the future, this may take into account an optional config file as
    /// well.
    #[allow(clippy::new_without_default)]
    pub fn new() -> Result<Self> {
        envy::from_env().context("error building config from env")
    }

    // /// The cluster's TLS config, if any.
    // pub fn tls_config(&self) -> Option<()> {
    //     None
    // }

    /// Parse the encoding key from the config source.
    fn parse_encoding_key<'de, D: Deserializer<'de>>(val: D) -> Result<EncodingKey, D::Error> {
        let b64_bytes: String = Deserialize::deserialize(val)?;
        let bytes = base64::decode(&b64_bytes).map_err(|err| DeError::custom(err.to_string()))?;
        EncodingKey::from_rsa_pem(&bytes).map_err(|err| DeError::custom(err.to_string()))
    }

    /// Parse the decoding key from the config source.
    fn parse_decoding_key<'de, D: Deserializer<'de>>(val: D) -> Result<(DecodingKey<'static>, String), D::Error> {
        let b64_bytes: String = Deserialize::deserialize(val)?;
        let bytes = base64::decode(&b64_bytes).map_err(|err| DeError::custom(err.to_string()))?;
        let key = DecodingKey::from_rsa_pem(&bytes)
            .map_err(|err| DeError::custom(err.to_string()))
            .map(|val| val.into_static())?;
        Ok((key, b64_bytes))
    }
}
