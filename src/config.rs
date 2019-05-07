use envy;
use serde::{Deserialize};

#[derive(Clone, Debug, Deserialize)]
pub struct Config {
    /// The base DNS name to use for discoverying peers via DNS.
    pub discovery_dns_name: String,
}

impl Config {
    /// Create a new config instance by parsing the current environment variables.
    ///
    /// Typically only one config instance should be created and then passed around as needed.
    pub fn new() -> Self {
        match envy::from_env() {
            Err(err) => panic!("{:?}", err),
            Ok(config) => config,
        }
    }
}
