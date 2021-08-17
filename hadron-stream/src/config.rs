//! Runtime configuration.

// TODO: finish up TLS config.

use anyhow::{Context, Result};
use jsonwebtoken::DecodingKey;
use serde::de::Error as DeError;
use serde::{Deserialize, Deserializer};

/// Runtime configuration data.
#[derive(Clone, Debug, Deserialize)]
pub struct Config {
    /// The server's logging config, which uses Rust's `env_logger` directives.
    pub rust_log: String,
    /// The port which client network traffic is to use.
    pub client_port: u16,
    /// The port which cluster internal network traffic is to use.
    pub server_port: u16,

    /// The CloudEvents root `source` of all events of this cluster.
    ///
    /// This value is used as the prefix of the `source` field of all events published to
    /// this stream, formatted as `{cluster_name}/{stream}/{partition}`.
    pub cluster_name: String,
    /// The Kubernetes namespace of this cluster.
    pub namespace: String,
    /// The name of this controller's stream.
    pub stream: String,
    /// The name of the statefulset to which this pod belongs.
    pub statefulset: String,
    /// The name of the pod on which this instance is running.
    pub pod_name: String,
    /// The partition of this pod.
    ///
    /// This value is derived from the `pod_name` value.
    #[serde(skip, default)]
    pub partition: u32,

    /// The path to the database on disk.
    #[serde(default = "crate::database::default_data_path")]
    pub storage_data_path: String,

    /// The JWT decoding key.
    #[serde(deserialize_with = "Config::parse_decoding_key")]
    pub jwt_decoding_key: DecodingKey<'static>,
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
        let mut config: Config = envy::from_env().context("error building config from env")?;
        config.partition = config
            .pod_name
            .split('-')
            .last()
            .and_then(|offset_str| offset_str.parse().ok())
            .context("invalid pod name, expected offset suffix at the end of the name")?;
        Ok(config)
    }

    // /// The cluster's TLS config, if any.
    // pub fn tls_config(&self) -> Option<()> {
    //     None
    // }

    /// Parse the decoding key from the config source.
    fn parse_decoding_key<'de, D: Deserializer<'de>>(val: D) -> Result<DecodingKey<'static>, D::Error> {
        let b64_bytes: String = Deserialize::deserialize(val)?;
        let bytes = base64::decode(&b64_bytes).map_err(|err| DeError::custom(err.to_string()))?;
        DecodingKey::from_rsa_pem(&bytes)
            .map_err(|err| DeError::custom(err.to_string()))
            .map(|val| val.into_static())
    }
}
