//! Runtime configuration.

use std::io::BufReader;

use anyhow::{Context, Result};
use serde::de::Error as DeError;
use serde::{Deserialize, Deserializer};
use tokio_rustls::rustls::{internal::pemfile, Certificate, PrivateKey};

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

    /// The webhook server's TLS certificate, PEM encoded.
    #[serde(deserialize_with = "Config::parse_webhook_cert")]
    pub webhook_cert: (Vec<Certificate>, String),
    /// The webhook server's TLS private key, PEM encoded.
    #[serde(deserialize_with = "Config::parse_webhook_key")]
    pub webhook_key: (PrivateKey, String),
}

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

    /// Parse the given base64 encoded webhook cert.
    fn parse_webhook_cert<'de, D: Deserializer<'de>>(val: D) -> Result<(Vec<Certificate>, String), D::Error> {
        let pem_cert: String = Deserialize::deserialize(val).map_err(|err| DeError::custom(format!("error parsing WEBHOOK_CERT: {}", err)))?;
        let certs = pemfile::certs(&mut BufReader::new(pem_cert.as_bytes())).map_err(|_err| DeError::custom("error parsing WEBHOOK_CERT"))?;
        if certs.is_empty() {
            Err(DeError::custom("no valid certs found in WEBHOOK_CERT"))
        } else {
            Ok((certs, pem_cert))
        }
    }

    /// Parse the given base64 encoded webhook key.
    fn parse_webhook_key<'de, D: Deserializer<'de>>(val: D) -> Result<(PrivateKey, String), D::Error> {
        let pem_key: String = Deserialize::deserialize(val).map_err(|err| DeError::custom(format!("error parsing WEBHOOK_KEY: {}", err)))?;

        let mut keys = pemfile::rsa_private_keys(&mut BufReader::new(pem_key.as_bytes()))
            .or_else(|_err| pemfile::pkcs8_private_keys(&mut BufReader::new(pem_key.as_bytes())))
            .map_err(|_err| DeError::custom("could not parse WEBHOOK_KEY as PKCS#1 or PKCS#8"))?;
        match keys.pop() {
            Some(key) => Ok((key, pem_key)),
            None => Err(DeError::custom("no private keys found in WEBHOOK_KEY")),
        }
    }
}
