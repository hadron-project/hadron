//! Runtime configuration.

// TODO:
// - prefix config vars with their config categroy.
// - add config for cluster auto heal. Cluster will only prune dead nodes if it is safe to do so.
// - finish up TLS config.

use std::time::Duration;

use serde::Deserialize;
use serde_aux::prelude::*;

/// Runtime configuration data.
#[derive(Clone, Debug, Deserialize)]
pub struct Config {
    /// The server's logging config, which uses Rust's `env_logger` directives.
    pub rust_log: String,
    /// The port which client network traffic is to use.
    pub client_port: u16,
    /// The port which cluster internal network traffic is to use.
    pub server_port: u16,

    /// The rate at which Raft should send heartbeats to connected peers, in milliseconds.
    pub raft_heartbeat_interval_millis: u16,
    /// The maximum amount of time for a Raft node to wait before triggering an election.
    pub raft_election_timeout_max: u16,
    /// The minimum amount of time for a Raft node to wait before triggering an election.
    pub raft_election_timeout_min: u16,

    /// The path to the database on disk.
    #[serde(default = "crate::database::default_data_path")]
    pub storage_data_path: String,

    /// The number of seconds to wait before issuing the initial cluster formation commands.
    initial_cluster_formation_delay: u8,
    _initial_cluster_formation_delay: Option<Duration>,
    /// The discovery backend to use.
    #[serde(flatten)]
    pub discovery_backend: DiscoveryBackend,

    /// The threshold in seconds of not receiving a heartbeat from a client, after which it is reckoned to be dead.
    #[serde(default = "Config::default_client_liveness_threshold")]
    client_liveness_threshold: u8,
    _client_liveness_threshold: Option<Duration>,
}

/// All available discovery backends currently implemented in this system.
#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "discovery_backend", rename_all = "UPPERCASE")]
pub enum DiscoveryBackend {
    /// The DNS discovery backend.
    Dns {
        /// The base DNS name to use for discoverying peers via DNS.
        discovery_dns_name: String,
        /// The interval in seconds which the DNS backend will poll for DNS updates.
        #[serde(deserialize_with = "deserialize_number_from_string")]
        discovery_dns_interval: u16,
    },
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
        let mut config: Config = match envy::from_env() {
            Err(err) => {
                tracing::error!(error = %err,  "error processing runtime config");
                std::thread::sleep(std::time::Duration::from_secs(5)); // Just give a little time to see the error before bailing.
                std::process::exit(1);
            }
            // Ok(config) => config,
            Ok(config) => {
                tracing::info!(?config, "runtime configuration");
                config
            }
        };
        config._client_liveness_threshold = Some(Duration::from_secs(config.client_liveness_threshold as u64));
        config._initial_cluster_formation_delay = Some(Duration::from_secs(config.initial_cluster_formation_delay as u64));
        config
    }

    /// The threshold in seconds of not receiving a heartbeat from a client, after which it is reckoned to be dead.
    ///
    /// This is the default value used for new client connections. Clients can override this
    /// value as needed during the client handshake.
    pub fn client_liveness_threshold(&self) -> Duration {
        self._client_liveness_threshold
            .unwrap_or_else(|| Duration::from_secs(self.client_liveness_threshold as u64))
    }

    /// The number of seconds to wait before issuing the initial cluster formation commands.
    pub fn initial_cluster_formation_delay(&self) -> Duration {
        self._initial_cluster_formation_delay
            .clone()
            .unwrap_or_else(|| Duration::from_secs(self.initial_cluster_formation_delay as u64))
    }

    /// The cluster's TLS config, if any.
    pub fn tls_config(&self) -> Option<()> {
        None
    }

    fn default_client_liveness_threshold() -> u8 {
        5
    }
}
