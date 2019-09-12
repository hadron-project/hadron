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

mod dns;
mod observedset;

use std::{
    sync::Arc,
};

use actix::prelude::*;
use log::{error};

use crate::{
    config::{Config, DiscoveryBackend},
    discovery::{
        dns::DnsAddrs,
        observedset::{ObservedSet},
    },
};
pub use observedset::ObservedPeersChangeset;

/// An internal type used for tracking the addr of the configured discovery backend.
enum DiscoveryBackendAddr {
    Dns(Addr<dns::DnsDiscovery>),
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// Discovery Actor ///////////////////////////////////////////////////////////////////////////////

/// An actor which provides a uniform interface to the peer discovery system.
///
/// See the README.md in this directory for additional information on actor responsibilities.
pub struct Discovery {

    /// The observed set of IP addresses corrently being tracked by this system.
    ///
    /// This field is used for tracking the IP addresses observed from the discovery loop of a
    /// discovery backend. IPs will be added to the system the first time they are observed by the
    /// backend, but IPs will not be removed until they've been omitted from a discovery cycle 3
    /// times.
    observed_peers: ObservedSet,

    /// The configured recipient to receive changeset notifications.
    out: Recipient<ObservedPeersChangeset>,

    _config: Arc<Config>,
    _backend_addr: DiscoveryBackendAddr,
}

impl Discovery {
    /// Create a new discovery instance configured to use the specified backend.
    pub fn new(ctx: &mut Context<Self>, out: Recipient<ObservedPeersChangeset>, config: Arc<Config>) -> Self {
        let observed_peers = ObservedSet::default();
        let _backend_addr = match &config.discovery_backend {
            DiscoveryBackend::Dns{discovery_dns_name} => {
                DiscoveryBackendAddr::Dns(dns::DnsDiscovery::new(
                    ctx.address(),
                    discovery_dns_name.clone(),
                    config.clone(),
                ).start())
            }
        };
        Self{observed_peers, out, _config: config, _backend_addr}
    }
}

impl Actor for Discovery {
    type Context = Context<Self>;
}

impl Handler<DnsAddrs> for Discovery {
    type Result = ();

    /// Handle messages coming from the DNS discovery backend.
    fn handle(&mut self, new_addrs: DnsAddrs, _: &mut Self::Context) -> Self::Result {
        // Update our internally observed set of peers, which produces a changeset.
        let changeset_opt = self.observed_peers.update_from_discovery_cycle(new_addrs.0);

        // Pump this changeset out to any registered subscribers.
        if let Some(changeset) = changeset_opt {
            let _ = self.out.do_send(changeset.clone())
                .map_err(|err| error!("Error delivering discovery changeset to subscriber. {}", err));
        }
    }
}
