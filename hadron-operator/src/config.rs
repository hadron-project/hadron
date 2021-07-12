//! Runtime configuration.

// TODO: finish up TLS config.

use std::sync::Arc;

use jsonwebtoken::{DecodingKey, EncodingKey};
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

    /// The name of this node's cluster.
    pub cluster: String,
    /// The Kubernetes namespace of this cluster.
    pub namespace: String,
    /// The name of the pod on which this instance is running.
    #[serde(deserialize_with = "Config::pod_name")]
    pub pod_name: Arc<String>,

    /// The duration in seconds for which a lease is considered held.
    ///
    /// To ensure stable cluster leadership, a 60 second lease is currently recommended.
    pub lease_duration_seconds: u32,
    /// The duration that a lease holder will retry refreshing lease.
    ///
    /// To ensure stable cluster leadership, a 10 seconds renew rate is currently recommended.
    pub lease_renew_seconds: u32,

    /// The path to the database on disk.
    #[serde(default = "crate::database::default_data_path")]
    pub storage_data_path: String,

    /// The JWT encoding key.
    #[serde(deserialize_with = "Config::parse_encoding_key")]
    pub jwt_encoding_key: EncodingKey,
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
    pub fn new() -> Self {
        let config: Config = match envy::from_env() {
            Err(err) => {
                tracing::error!(error = %err,  "error building config from env");
                std::thread::sleep(std::time::Duration::from_secs(5)); // Just give a little time to see the error before bailing.
                std::process::exit(1);
            }
            Ok(config) => config,
        };
        config
    }

    /// The cluster's TLS config, if any.
    pub fn tls_config(&self) -> Option<()> {
        None
    }

    /// Parse the decoding key from the config source.
    fn parse_encoding_key<'de, D: Deserializer<'de>>(val: D) -> Result<EncodingKey, D::Error> {
        let b64_bytes: String = Deserialize::deserialize(val)?;
        let bytes = base64::decode(&b64_bytes).map_err(|err| DeError::custom(err.to_string()))?;
        EncodingKey::from_rsa_pem(&bytes).map_err(|err| DeError::custom(err.to_string()))
    }

    /// Parse the decoding key from the config source.
    fn parse_decoding_key<'de, D: Deserializer<'de>>(val: D) -> Result<DecodingKey<'static>, D::Error> {
        let b64_bytes: String = Deserialize::deserialize(val)?;
        let bytes = base64::decode(&b64_bytes).map_err(|err| DeError::custom(err.to_string()))?;
        DecodingKey::from_rsa_pem(&bytes)
            .map_err(|err| DeError::custom(err.to_string()))
            .map(|val| val.into_static())
    }

    fn pod_name<'de, D: Deserializer<'de>>(val: D) -> Result<Arc<String>, D::Error> {
        let val: String = Deserialize::deserialize(val)?;
        Ok(Arc::new(val))
    }
}
