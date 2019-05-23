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
use serde::Deserialize;

use crate::{
    config::Config,
    discovery::{
        dns::DnsAddrs,
        observedset::{ObservedSet},
    },
};
pub use observedset::ObservedPeersChangeset;

/// All available discovery backends currently implemented in this system.
#[derive(Clone, Debug, Deserialize)]
pub enum DiscoveryBackend {
    Dns,
}

/// An internal type used for tracking the addr of the configured discovery backend.
enum DiscoveryBackendAddr {
    Dns(Addr<dns::DnsDiscovery>),
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// Message Types /////////////////////////////////////////////////////////////////////////////////

/// A message for subscribing to changesets coming from the discovery system.
#[derive(Message)]
pub struct SubscribeToDiscoveryChangesets(pub Recipient<ObservedPeersChangeset>);

//////////////////////////////////////////////////////////////////////////////////////////////////
// Discovery Actor ///////////////////////////////////////////////////////////////////////////////

/// An actor which provides a uniform interface to the peer discovery system.
///
/// See the README.md in this directory for additional information on actor responsibilities.
pub struct Discovery {
    backend: DiscoveryBackend,
    backend_addr: Option<DiscoveryBackendAddr>, // Once the actor starts, this will be safe to unwrap.
    config: Arc<Config>,

    /// The observed set of IP addresses corrently being tracked by this system.
    ///
    /// This field is used for tracking the IP addresses observed from the discovery loop of a
    /// discovery backend. IPs will be added to the system the first time they are observed by the
    /// backend, but IPs will not be removed until they've been omitted from a discovery cycle 3
    /// times.
    observed_peers: ObservedSet,

    /// All registered subscribers to changeset events from this system.
    subscribers: Vec<Recipient<ObservedPeersChangeset>>,
}

impl Discovery {
    /// Create a new discovery instance configured to use the specified backend.
    pub fn new(backend: DiscoveryBackend, config: Arc<Config>) -> Self {
        let observed_peers = ObservedSet::default();
        Self{backend, backend_addr: None, config, observed_peers, subscribers: vec![]}
    }
}

impl Actor for Discovery {
    type Context = Context<Self>;

    /// Logic for starting this actor.
    fn started(&mut self, ctx: &mut Self::Context) {
        use DiscoveryBackend::*;
        let backend_addr = match self.backend {
            Dns => DiscoveryBackendAddr::Dns(dns::DnsDiscovery::new(ctx.address(), self.config.clone()).start()),
        };
        self.backend_addr = Some(backend_addr);
    }
}

impl Handler<DnsAddrs> for Discovery {
    type Result = ();

    /// Handle messages coming from the DNS discovery backend.
    fn handle(&mut self, new_addrs: DnsAddrs, _: &mut Self::Context) -> Self::Result {
        // Update our internally observed set of peers, which produces a changeset.
        // TODO: update changesets to include snapshots for reconciliation.
        let changeset_opt = self.observed_peers.update_from_discovery_cycle(new_addrs.0);

        // Pump this changeset out to any registered subscribers.
        if let Some(changeset) = changeset_opt {
            self.subscribers.iter().for_each(|addr: &Recipient<ObservedPeersChangeset>| {
                let _ = addr.do_send(changeset.clone())
                    .map_err(|err| error!("Error delivering discovery changeset to subscriber. {}", err));
            })
        }
    }
}

impl Handler<SubscribeToDiscoveryChangesets> for Discovery {
    type Result = ();

    /// Handle requests to subscribe to discovery changesets.
    fn handle(&mut self, subscriber: SubscribeToDiscoveryChangesets, _: &mut Self::Context) -> Self::Result {
        self.subscribers.push(subscriber.0);
    }
}
