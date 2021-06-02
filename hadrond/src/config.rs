//! Runtime configuration.

// TODO:
// - prefix config vars with their config categroy.
// - finish up TLS config.
// - finish up cluster level config.
// - finish up replica discovery mechanism.

use jsonwebtoken::{DecodingKey, EncodingKey};
use serde::de::{value::BorrowedBytesDeserializer, DeserializeOwned, Error as DeError};
use serde::{Deserialize, Deserializer};
// use serde_aux::prelude::*; // Will use later.

/// Runtime configuration data.
#[derive(Clone, Debug, Deserialize)]
pub struct Config {
    /// The server's logging config, which uses Rust's `env_logger` directives.
    pub rust_log: String,
    /// The port which client network traffic is to use.
    pub client_port: u16,

    /// The name of this node.
    pub node_name: String,
    /// The name of this node's replica set.
    pub repl_set_name: String,
    /// The name of the node which is the leader of this replica set.
    pub leader_name: String,
    /// The name of the replica set which is responsible for cluster metadata.
    pub metadata_repl_set_name: String,

    /// The path to the database on disk.
    #[serde(default = "crate::database::default_data_path")]
    pub storage_data_path: String,

    /// Configuration for the cluster's metadata replica set.
    #[serde(flatten)]
    pub metadata_config: MetadataConfig,
}

/// Runtime configuration specifically for a cluster's metadata replica set.
#[derive(Clone, Debug, Deserialize)]
pub struct MetadataConfig {
    /// The JWT encoding key.
    #[serde(deserialize_with = "MetadataConfig::parse_encoding_key")]
    pub jwt_encoding_key: EncodingKey,
    /// The JWT decoding key.
    #[serde(deserialize_with = "MetadataConfig::parse_decoding_key")]
    pub jwt_decoding_key: DecodingKey<'static>,
}

impl MetadataConfig {
    /// Parse the decoding key from the config source.
    fn parse_encoding_key<'de, D: Deserializer<'de>>(val: D) -> Result<EncodingKey, D::Error> {
        let bytes: String = Deserialize::deserialize(val)?;
        EncodingKey::from_rsa_pem(bytes.as_bytes()).map_err(|err| DeError::custom(err.to_string()))
    }

    /// Parse the decoding key from the config source.
    fn parse_decoding_key<'de, D: Deserializer<'de>>(val: D) -> Result<DecodingKey<'static>, D::Error> {
        let bytes: String = Deserialize::deserialize(val)?;
        DecodingKey::from_rsa_pem(bytes.as_bytes())
            .map_err(|err| DeError::custom(err.to_string()))
            .map(|val| val.into_static())
    }
}

// /// The discovery backend to use.
// #[serde(flatten)]
// pub discovery_backend: DiscoveryBackend,
//
// /// All available discovery backends currently implemented in this system.
// #[derive(Clone, Debug, Deserialize)]
// #[serde(tag = "discovery_backend", rename_all = "UPPERCASE")]
// pub enum DiscoveryBackend {
//     /// The DNS discovery backend.
//     Dns {
//         /// The base DNS name to use for discoverying peers via DNS.
//         discovery_dns_name: String,
//         /// The interval in seconds which the DNS backend will poll for DNS updates.
//         #[serde(deserialize_with = "deserialize_number_from_string")]
//         discovery_dns_interval: u16,
//     },
// }

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
}
