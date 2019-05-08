use std::{
    sync::Arc,
};

use actix::prelude::*;
use log::{info};

use crate::{
    config::Config,
    discovery::{Discovery, DiscoveryBackend},
};

pub struct App;

impl App {
    /// Create a new instance.
    pub fn new() -> Self {
        App
    }

    /// Run the application.
    pub fn run(self) {
        // Build the system arbiter.
        let sys = actix::System::new("railgun");

        // Parse runtime config.
        let config = Arc::new(Config::new());

        // Boot the configured discovery system.
        // TODO: use runtime config for passing in the selected discovery backend.
        let _discovery_addr = Discovery::new(DiscoveryBackend::Dns, config.clone()).start();

        info!("Running railgun.");
        let _ = sys.run(); // This blocks. Actix automatically handles unix signals for termination & graceful shutdown.
    }
}
