use std::{
    sync::Arc,
};

use actix::prelude::*;
use log::{info};
use uuid;

use crate::{
    config::Config,
    connections::{Connections},
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
        let (discovery_arb, discovery_cfg) = (Arbiter::new(), config.clone());
        let discovery_addr = Discovery::start_in_arbiter(&discovery_arb, move |_| Discovery::new(DiscoveryBackend::Dns, discovery_cfg));

        // Boot the connections actor. Its network server will operate on dedicated threads.
        let node_id = uuid::Uuid::new_v4().to_string(); // TODO: this should come from the DB layer.
        info!("Node ID is: {}", &node_id); // TODO: rm this.
        let _conns_addr = Connections::new(discovery_addr.clone(), node_id, config.clone()).start();

        info!("Railgun is firing on port '{}'.", config.port);
        let _ = sys.run(); // This blocks. Actix automatically handles unix signals for termination & graceful shutdown.
    }
}
