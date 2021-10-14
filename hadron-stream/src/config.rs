//! Runtime configuration.

use std::sync::Arc;

use anyhow::{Context, Result};
use serde::Deserialize;

/// Runtime configuration data.
#[derive(Clone, Debug, Deserialize)]
pub struct Config {
    /// The server's logging config, which uses Rust's `env_logger` directives.
    pub rust_log: String,
    /// The port which client network traffic is to use.
    pub client_port: u16,
    /// The port which cluster internal network traffic is to use.
    pub server_port: u16,

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
}

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

    /// Build an instance for use in tests.
    #[cfg(test)]
    pub fn new_test() -> Result<(Arc<Self>, tempfile::TempDir)> {
        let tmpdir = tempfile::tempdir_in("/tmp").context("error creating tmp dir in /tmp")?;
        Ok((
            Arc::new(Self {
                rust_log: "".into(),
                client_port: 7000,
                server_port: 7000,

                namespace: "default".into(),
                stream: "testing".into(),
                statefulset: "testing".into(),
                pod_name: "testing-0".into(),
                partition: 0,

                storage_data_path: tmpdir.path().to_string_lossy().to_string(),
            }),
            tmpdir,
        ))
    }
}
