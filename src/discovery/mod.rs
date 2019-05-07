//! Peer discovery actor abstraction.
//!
//! This module provides an abstraction over the peer discovery system. The interface here is
//! quite simple. All possible discovery actors implement the `Discovery` trait. Based on the
//! runtime configuration for this system, the appropriate discovery actor will be created using
//! this modules `new_discovery_instance` function. The returned object should be used for
//! registering listener to observe peer discovery changes.
//!
//! The discovery actors are stream only actors. They do not expect any input from other actors in
//! this system. Other actors which need to observe the stream of changes coming from this actor
//! should subscribe to this actor.

pub mod dns;

use std::{
    sync::Arc,
};

use actix::prelude::*;

use crate::{
    config::Config,
};

/// All available discovery backends currently implemented in this system.
#[derive(Clone, Debug)]
pub enum DiscoveryBackend {
    Dns,
}

/// An internal type used for tracking the addr of the configured discovery backend.
enum DiscoveryBackendAddr {
    Dns(Addr<dns::DnsDiscovery>),
}

/// An actor which provides a uniform interface to the peer discovery system.
pub struct Discovery {
    backend_addr: DiscoveryBackendAddr,
    config: Arc<Config>,
}

impl Discovery {
    /// Create a new discovery instance configured to use the specified backend.
    pub fn new(backend: DiscoveryBackend, config: Arc<Config>) -> Self {
        use DiscoveryBackend::*;
        let backend_addr = match backend {
            Dns => DiscoveryBackendAddr::Dns(dns::DnsDiscovery::new(config.clone()).start()),
        };

        Self{backend_addr, config}
    }
}

impl Actor for Discovery {
    type Context = Context<Self>;
}
