use std::{
    path,
    process,
};

use envy;
use log::error;
use serde::{Deserialize};

use crate::db;

/// Runtime configuration for Railgun.
#[derive(Clone, Debug, Deserialize)]
pub struct Config {
    /// The port which this instance has been configured to communicate on.
    pub port: u16,

    /// The discovery backend to use.
    #[serde(flatten)]
    pub discovery_backend: DiscoveryBackend,

    /// The path to the database on disk.
    #[serde(default="db::default_db_path")]
    pub db_path: String,

    /// The path to the configured snapshots dir.
    #[serde(skip)]
    snapshot_dir: Option<String>,
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
        config.snapshot_dir = Some(config.build_snapshot_dir());
        config
    }

    /// The path to the configured snapshots dir.
    pub fn snapshot_dir(&self) -> String {
        self.snapshot_dir.clone().unwrap_or_else(|| self.build_snapshot_dir())
    }

    fn build_snapshot_dir(&self) -> String {
        path::PathBuf::from(self.db_path.clone()).join(db::DEFAULT_SNAPSHOT_SUBDIR)
            .to_string_lossy().to_string()
    }
}
