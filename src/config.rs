//! TODO:
//! - remove RG_ prefix.
//! - prefix config vars with their config categroy.

use std::{
    path,
    process,
    time::Duration,
};

use envy;
use log::error;
use serde::{Deserialize};

use crate::db;

/// Runtime configuration for Railgun.
#[derive(Clone, Debug, Deserialize)]
pub struct Config {
    //////////////////////////////////////////////////////////////////////////
    // Server Config /////////////////////////////////////////////////////////

    /// The port which this instance has been configured to communicate on.
    pub port: u16,

    /// The server's logging config. Uses Rust's `env_logger` directives.
    pub logging: String,

    //////////////////////////////////////////////////////////////////////////
    // Raft Config ///////////////////////////////////////////////////////////

    /// The rate at which Raft should send heartbeats to connected peers, in milliseconds.
    pub raft_heartbeat_interval_millis: u16,

    /// The maximum amount of time for a Raft node to wait before triggering an election.
    pub raft_election_timeout_max: u16,

    /// The minimum amount of time for a Raft node to wait before triggering an election.
    pub raft_election_timeout_min: u16,

    //////////////////////////////////////////////////////////////////////////
    // Storage Config ////////////////////////////////////////////////////////

    /// The path to the database on disk.
    #[serde(default="db::default_db_path")]
    pub storage_db_path: String,

    /// The path to the configured snapshots dir.
    #[serde(skip)]
    storage_snapshot_dir: Option<String>,

    //////////////////////////////////////////////////////////////////////////
    // Clustering Config /////////////////////////////////////////////////////

    /// The number of seconds to wait before issuing the initial cluster formation commands.
    initial_cluster_formation_delay: u8,
    _initial_cluster_formation_delay: Option<Duration>,

    /// The discovery backend to use.
    #[serde(flatten)]
    pub discovery_backend: DiscoveryBackend,

    //////////////////////////////////////////////////////////////////////////
    // Client Config /////////////////////////////////////////////////////////

    /// The threshold in seconds of not receiving a heartbeat from a client, after which it is reckoned to be dead.
    #[serde(default="Config::default_client_liveness_threshold")]
    client_liveness_threshold: u8,
    _client_liveness_threshold: Option<Duration>,
}

/// All available discovery backends currently implemented in this system.
#[derive(Clone, Debug, Deserialize)]
#[serde(tag="discovery_backend", rename_all="UPPERCASE")]
pub enum DiscoveryBackend {
    /// The DNS discovery backend.
    Dns {
        /// The base DNS name to use for discoverying peers via DNS.
        discovery_dns_name: String,
    }
}

impl Config {
    /// Create a new config instance.
    ///
    /// Currently this routing just parses the runtime environment and builds the application
    /// config from that. In the future, this may take into account an optional config file as
    /// well.
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        let mut config: Config = match envy::prefixed("RG_").from_env() {
            Err(err) => {
                error!("{:?}", err);
                process::exit(1);
            },
            // Ok(config) => config,
            Ok(config) => {
                println!("Config: {:?}", config);
                config
            }
        };
        config.storage_snapshot_dir = Some(Config::build_snapshot_dir(config.storage_db_path.clone()));
        config._client_liveness_threshold = Some(Duration::from_secs(config.client_liveness_threshold as u64));
        config._initial_cluster_formation_delay = Some(Duration::from_secs(config.initial_cluster_formation_delay as u64));
        config
    }

    /// The threshold in seconds of not receiving a heartbeat from a client, after which it is reckoned to be dead.
    ///
    /// This is the default value used for new client connections. Clients can override this
    /// value as needed during the client handshake.
    pub fn client_liveness_threshold(&self) -> Duration {
        self._client_liveness_threshold.unwrap_or_else(|| Duration::from_secs(self.client_liveness_threshold as u64))
    }

    /// The number of seconds to wait before issuing the initial cluster formation commands.
    pub fn initial_cluster_formation_delay(&self) -> Duration {
        self._initial_cluster_formation_delay.clone().unwrap_or_else(|| Duration::from_secs(self.initial_cluster_formation_delay as u64))
    }

    /// The path to the configured snapshots dir.
    pub fn storage_snapshot_dir(&self) -> String {
        self.storage_snapshot_dir.clone().unwrap_or_else(|| Config::build_snapshot_dir(self.storage_db_path.clone()))
    }

    //////////////////////////////////////////////////////////////////////////
    // Private Methods ///////////////////////////////////////////////////////

    fn build_snapshot_dir(db_path: String) -> String {
        path::PathBuf::from(db_path).join(db::DEFAULT_SNAPSHOT_SUBDIR).to_string_lossy().to_string()
    }

    fn default_client_liveness_threshold() -> u8 {
        5
    }
}
