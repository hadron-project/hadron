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

    //////////////////////////////////////////////////////////////////////////
    // Discovery Config //////////////////////////////////////////////////////

    /// The discovery backend to use.
    #[serde(flatten)]
    pub discovery_backend: DiscoveryBackend,

    //////////////////////////////////////////////////////////////////////////
    // Storage Config ////////////////////////////////////////////////////////

    /// The path to the database on disk.
    #[serde(default="db::default_db_path")]
    pub db_path: String,

    /// The path to the configured snapshots dir.
    #[serde(skip)]
    snapshot_dir: Option<String>,

    //////////////////////////////////////////////////////////////////////////
    // Client Config /////////////////////////////////////////////////////////

    /// The threshold in seconds of not receiving a heartbeat from a client, after which it is reckoned to be dead.
    #[serde(default="Config::default_client_death_threshold")]
    client_death_threshold: u8,
    _client_death_threshold: Option<Duration>,
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
        config.snapshot_dir = Some(Config::build_snapshot_dir(config.db_path.clone()));
        config._client_death_threshold = Some(Duration::from_secs(config.client_death_threshold as u64));
        config
    }

    /// The path to the configured snapshots dir.
    pub fn snapshot_dir(&self) -> String {
        self.snapshot_dir.clone().unwrap_or_else(|| Config::build_snapshot_dir(self.db_path.clone()))
    }

    pub fn client_death_threshold(&self) -> Duration {
        self._client_death_threshold.unwrap_or_else(|| Duration::from_secs(self.client_death_threshold as u64))
    }
}

impl Config {
    fn build_snapshot_dir(db_path: String) -> String {
        path::PathBuf::from(db_path).join(db::DEFAULT_SNAPSHOT_SUBDIR).to_string_lossy().to_string()
    }

    fn default_client_death_threshold() -> u8 {
        5
    }
}
