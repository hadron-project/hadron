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

        // Boot the configured discovery system on a new dedicated thread.
        // NOTE: currently we only support DNS discovery, so its selection is hard-coded.
        let (discovery_arbiter, discovery_cfg) = (Arbiter::new(), config.clone());
        let _discovery_addr = Discovery::start_in_arbiter(&discovery_arbiter, move |_| Discovery::new(DiscoveryBackend::Dns, discovery_cfg));

        info!("Railgun is firing on port '{}'.", config.port);
        let _ = sys.run(); // This blocks. Actix automatically handles unix signals for termination & graceful shutdown.
    }
}
