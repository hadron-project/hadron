use std::process;

use envy;
use log::error;
use serde::{Deserialize};

use crate::db::Database;

#[derive(Clone, Debug, Deserialize)]
pub struct Config {
    /// The port which this instance has been configured to communicate on.
    pub port: u16,

    /// The base DNS name to use for discoverying peers via DNS.
    pub discovery_dns_name: String,

    /// The path to the database on disk.
    #[serde(default="Database::default_db_path")]
    pub db_path: String,
}

impl Config {
    /// Create a new config instance.
    ///
    /// Currently this routing just parses the runtime environment and builds the application
    /// config from that. In the future, this may take into account an optional config file as
    /// well.
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        match envy::prefixed("RG_").from_env() {
            Err(err) => {
                error!("{:?}", err);
                process::exit(1);
            },
            Ok(config) => config,
        }
    }
}
