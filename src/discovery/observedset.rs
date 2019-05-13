use std::{
    collections::HashSet,
    net::SocketAddr,
};

use actix::prelude::*;
use log::{debug};

/// A type representing some set of changes to the observed set of discovered peers.
#[derive(Clone, Debug, Message)]
pub struct ObservedPeersChangeset {
    pub new_peers: Vec<SocketAddr>,
    pub purged_peers: Vec<SocketAddr>,
}

/// A type used for encapsulated the logic for tracking an observed set of peer addrs.
#[derive(Default)]
pub(super) struct ObservedSet(HashSet<SocketAddr>);

impl ObservedSet {
    /// Perform an update based on a set of peer addrs observed from a backend discovery cycle.
    ///
    /// This routine will add newly observed peers immediately to the set of observed peers. Peers
    /// currently in the observed set, but which are not in the given payload, will receive a
    /// demotion. After three conscutive demotions, the peer addr will be removed from the set.
    pub fn update_from_discovery_cycle(&mut self, addrs: Vec<SocketAddr>) -> Option<ObservedPeersChangeset> {
        let addrset: HashSet<_> = addrs.into_iter().collect();

        // Add newly observed peers to the observation set.
        let newaddrs: Vec<_> = addrset.difference(&self.0).cloned().collect();
        let new_peers: Vec<_> = newaddrs.into_iter()
            .inspect(|new| debug!("Adding peer at {:?} to observed set of peers.", &new))
            .map(|new| {
                self.0.insert(new.clone());
                new
            })
            .collect();

        // Remove peers from the observation set which did not appear in the most recent payload.
        let removal_targets: Vec<_> = self.0.difference(&addrset).cloned().collect();
        let purged_peers: Vec<_> = removal_targets.into_iter()
            .inspect(|elem| debug!("Purging peer {:?} from observed set.", elem))
            .map(|elem| {
                self.0.remove(&elem);
                elem
            }).collect();

        if new_peers.len() > 0 || purged_peers.len() > 0 {
            Some(ObservedPeersChangeset{new_peers, purged_peers})
        } else {
            None
        }
    }
}
