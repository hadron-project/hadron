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
    collections::{HashMap, HashSet},
    net::IpAddr,
    sync::Arc,
};

use actix::prelude::*;
use log::{debug};
use serde::Deserialize;

use crate::{
    config::Config,
    discovery::dns::DnsAddrs,
};

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
}

impl Discovery {
    /// Create a new discovery instance configured to use the specified backend.
    pub fn new(backend: DiscoveryBackend, config: Arc<Config>) -> Self {
        let observed_peers = ObservedSet::default();
        Self{backend, backend_addr: None, config, observed_peers}
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
        let _changeset = self.observed_peers.update_from_discovery_cycle(
            new_addrs.0.into_iter()
                .map(|addr| PeerAddr{addr, port: self.config.port})
                .collect()
        );

        // Pump this changeset out to any registered subscribers.
        // TODO: setup changeset subscription broadcasting.
    }
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// ObservedSet ///////////////////////////////////////////////////////////////////////////////////

/// A type wrapping an IpAddr and a port.
#[derive(Clone, Debug, Hash, Eq, PartialEq)]
pub struct PeerAddr {
    pub addr: IpAddr,
    pub port: u16,
}

/// A type used for encapsulated the logic for tracking an observed set of peer addrs.
#[derive(Default)]
struct ObservedSet {
    all: HashSet<PeerAddr>,
    demoting: HashMap<PeerAddr, u8>,
}

impl ObservedSet {
    /// Perform an update based on a set of peer addrs observed from a backend discovery cycle.
    ///
    /// This routine will add newly observed peers immediately to the set of observed peers. Peers
    /// currently in the observed set, but which are not in the given payload, will receive a
    /// demotion. After three conscutive demotions, the peer addr will be removed from the set.
    pub fn update_from_discovery_cycle(&mut self, addrs: Vec<PeerAddr>) -> Option<ObservedPeersChangeset> {
        debug!("Performing update on observed set of peer IP addrs.");
        let addrset: HashSet<_> = addrs.into_iter().collect();

        // Add newly observed peers to the observation set.
        let newaddrs: Vec<_> = addrset.difference(&self.all).cloned().collect();
        let new_peers: Vec<_> = newaddrs.into_iter()
            .inspect(|new| debug!("Adding peer at {:?} to observed set of peers.", &new))
            .map(|new| {
                self.all.insert(new.clone());
                new
            })
            .collect();

        // Check for demoting members which need to be restored. If they have
        // reappeared in the observation cycle, then remove them from the demotion pool.
        let demotingset: HashSet<_> = self.demoting.keys().cloned().collect();
        let restored: Vec<_> = addrset.intersection(&demotingset).cloned().collect();
        let _: () = restored.into_iter()
            .inspect(|elem| debug!("Restoring peer {:?} from demotion set.", elem))
            .map(|elem| { self.demoting.remove(&elem); })
            .collect();

        // Check for any members which need to be demoted.
        let demote_targets: Vec<_> = self.all.difference(&addrset).cloned().collect();
        let purge_targets = demote_targets.into_iter()
            .inspect(|elem| debug!("Demoting peer {:?}.", elem))
            .fold(vec![], |mut acc, elem| {
                self.demoting.entry(elem.clone())
                    // Check if peer is already being demoted. If so, increment its demotion
                    // count. If already at 3 demotions, add it to the purge set.
                    .and_modify(|current| if *current == 3 {
                        acc.push(elem);
                    } else {
                        *current += 1;
                    })

                    // Insert peer into demotion set with one demotion.
                    .or_insert(1);
                acc
            });

        // If there are peers which have been demoted more then 3 times now, purge them.
        let purged_peers: Vec<_> = purge_targets.into_iter()
            .inspect(|elem| debug!("Purging peer {:?} from observed set.", elem))
            .map(|elem| {
                self.all.remove(&elem);
                self.demoting.remove(&elem);
                elem
            }).collect();

        if new_peers.len() > 0 || purged_peers.len() > 0 {
            Some(ObservedPeersChangeset{new_peers, purged_peers})
        } else {
            None
        }
    }
}

/// A type representing some set of changes to the observed set of discovered peers.
pub struct ObservedPeersChangeset {
    pub new_peers: Vec<PeerAddr>,
    pub purged_peers: Vec<PeerAddr>,
}
