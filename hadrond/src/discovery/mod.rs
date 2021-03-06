//! Peer discovery.
//!
//! Discovery backends do not expect any input from other controllers. Other controllers which
//! need to observe the stream of changes coming from this controller should subscribe to this
//! controller's output stream.

mod dns;

use std::collections::HashSet;
use std::sync::Arc;

use tokio::stream::StreamExt;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use crate::config::{Config, DiscoveryBackend};

/// A controller which provides a uniform interface to the peer discovery system.
pub struct Discovery {
    /// The observed set of IP addresses currently being tracked by this system.
    ///
    /// This field is used for tracking the IP addresses observed from the discovery loop of a
    /// discovery backend. IPs will be added to the system the first time they are observed by the
    /// backend, but IPs will not be removed until they've been omitted from a discovery cycle a
    /// configurable number of times.
    observed_peers: ObservedSet,
    /// The configured recipient to receive changeset notifications.
    output_tx: mpsc::Sender<ObservedPeersChangeset>,
    rx_backend: watch::Receiver<Vec<PeerSrv>>,
    /// Application shutdown signal.
    shutdown: watch::Receiver<bool>,

    _config: Arc<Config>,
    backend_handle: JoinHandle<()>,
}

impl Discovery {
    /// Create a new discovery instance configured to use the specified backend.
    pub fn new(config: Arc<Config>, shutdown: watch::Receiver<bool>) -> (Self, mpsc::Receiver<ObservedPeersChangeset>) {
        let observed_peers = ObservedSet::default();
        let (output_tx, output_rx) = mpsc::channel(1);
        let (tx_backend, rx_backend) = tokio::sync::watch::channel(vec![]);
        let backend_handle = match &config.discovery_backend {
            DiscoveryBackend::Dns {
                discovery_dns_name,
                discovery_dns_interval,
            } => dns::DnsDiscovery::new(
                config.clone(),
                tx_backend,
                discovery_dns_name.clone(),
                *discovery_dns_interval,
                shutdown.clone(),
            )
            .spawn(),
        };
        let this = Self {
            observed_peers,
            output_tx,
            rx_backend,
            shutdown,
            _config: config,
            backend_handle,
        };
        (this, output_rx)
    }

    pub fn spawn(self) -> JoinHandle<()> {
        tokio::spawn(self.run())
    }

    async fn run(mut self) {
        loop {
            tokio::select! {
                Some(addrs) = self.rx_backend.next() => {
                    self.handle_discovery_output(addrs).await;
                }
                Some(needs_shutdown) = self.shutdown.next() => if needs_shutdown { break } else { continue },
            }
        }

        // Graceful shutdown.
        tracing::debug!("discovery is shutting down");
        let _ = self.backend_handle.await;
        tracing::debug!("discovery shutdown");
    }

    /// Handle messages coming from the DNS discovery backend.
    async fn handle_discovery_output(&mut self, new_addrs: Vec<PeerSrv>) {
        // Update our internally observed set of peers, which produces a changeset.
        let changeset_opt = self.observed_peers.update_from_discovery_cycle(new_addrs);
        if let Some(changeset) = changeset_opt {
            let _ = self.output_tx.send(changeset).await;
        }
    }
}

/// The SRV details of a peer node on the network.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct PeerSrv {
    /// The port on which the peer node receives connections.
    pub port: u16,
    /// The FQDN of the peer.
    pub fqdn: String,
}

/// A type representing some set of changes to the observed set of discovered peers.
#[derive(Clone, Debug)]
pub struct ObservedPeersChangeset {
    pub new_peers: Vec<PeerSrv>,
    pub purged_peers: Vec<PeerSrv>,
}

/// A type used for encapsulated the logic for tracking an observed set of peer addrs.
#[derive(Default)]
pub(super) struct ObservedSet(HashSet<PeerSrv>);

impl ObservedSet {
    /// Perform an update based on a set of peer addrs observed from a backend discovery cycle.
    ///
    /// This routine will add newly observed peers immediately to the set of observed peers. Peers
    /// currently in the observed set, but which are not in the given payload, will be categorized
    /// as purged.
    #[tracing::instrument(level = "trace", skip(self))]
    pub fn update_from_discovery_cycle(&mut self, addrs: Vec<PeerSrv>) -> Option<ObservedPeersChangeset> {
        let addrset: HashSet<_> = addrs.into_iter().collect();

        // Add newly observed peers to the observation set.
        let new_peers: Vec<_> = addrset.difference(&self.0).cloned().collect();
        for peer in new_peers.iter() {
            self.0.insert(peer.clone());
        }

        // Remove peers from the observation set which did not appear in the most recent payload.
        let purged_peers: Vec<_> = self.0.difference(&addrset).cloned().collect();
        for peer in purged_peers.iter() {
            self.0.remove(peer);
        }

        if !new_peers.is_empty() || !purged_peers.is_empty() {
            let changeset = ObservedPeersChangeset { new_peers, purged_peers };
            tracing::trace!(?changeset, "new changeset");
            Some(changeset)
        } else {
            None
        }
    }
}
