use std::{
    collections::{HashMap, HashSet},
};

use actix::prelude::*;
use log::{debug};

use crate::discovery::PeerAddr;

/// A type representing some set of changes to the observed set of discovered peers.
#[derive(Clone, Debug, Message)]
pub struct ObservedPeersChangeset {
    pub new_peers: Vec<PeerAddr>,
    pub purged_peers: Vec<PeerAddr>,
}

/// A type used for encapsulated the logic for tracking an observed set of peer addrs.
#[derive(Default)]
pub(super) struct ObservedSet {
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
