//! Discovery DNS backend.

use std::sync::Arc;

use tokio::sync::watch;
use tokio::task::JoinHandle;
use trust_dns_resolver::TokioAsyncResolver;

use crate::config::Config;
use crate::discovery::PeerSrv;

const DNS_SYS_CONF_ERR: &str = "failed to read system DNS config; on *nix systems, ensure your resolv.conf is present and properly formed";

/// An actor used for DNS based peer discovery.
///
/// This discovery system will check for new peers on a regular interval, emitting discovered
/// peers on a regular interval. This backend is dual-stack aware, and works for IPv4 & IPv6.
pub struct DnsDiscovery {
    _config: Arc<Config>,
    discovery_dns_name: String,
    discovery_dns_interval: u16,
    tx: watch::Sender<Vec<PeerSrv>>,
    /// Application shutdown signal.
    shutdown: watch::Receiver<bool>,
}

impl DnsDiscovery {
    /// Create a new DNS peer discovery backend instance.
    pub fn new(
        _config: Arc<Config>, tx: watch::Sender<Vec<PeerSrv>>, discovery_dns_name: String, discovery_dns_interval: u16,
        shutdown: watch::Receiver<bool>,
    ) -> Self {
        Self {
            _config,
            discovery_dns_name,
            discovery_dns_interval,
            tx,
            shutdown,
        }
    }

    pub fn spawn(self) -> JoinHandle<()> {
        tokio::spawn(self.run())
    }

    async fn run(self) {
        let resolver = self.build_resolver().await;
        self.discovery_loop(resolver).await;
        tracing::debug!("dns discovery shutdown");
    }

    async fn build_resolver(&self) -> TokioAsyncResolver {
        loop {
            match TokioAsyncResolver::tokio_from_system_conf().await {
                // If we've successfully built the resolver, then return it.
                Ok(resolver) => return resolver,
                // Else, we delay for a bit, and attempt to build it again by continuing the loop.
                Err(err) => {
                    tracing::error!(error = %err, "{}", DNS_SYS_CONF_ERR);
                    tokio::time::delay_for(std::time::Duration::from_secs(self.discovery_dns_interval as u64)).await;
                    continue;
                }
            };
        }
    }

    /// Execute the DNS discovery loop, and don't stop until the shutdown signal is received.
    async fn discovery_loop(&self, resolver: TokioAsyncResolver) {
        loop {
            if *self.shutdown.borrow() {
                return;
            }
            let discovery_fut = resolver.srv_lookup(self.discovery_dns_name.as_str());
            let timeout = std::time::Duration::from_secs(10);
            match tokio::time::timeout(timeout, discovery_fut).await {
                Err(err) => tracing::debug!(error = %err, "timeout during DNS discovery cycle"),
                Ok(Err(err)) => tracing::debug!(error = %err, "error during DNS discovery cycle"),
                Ok(Ok(srv_records)) => {
                    let addrs = srv_records
                        .into_iter()
                        .map(|srv| PeerSrv {
                            port: srv.port(),
                            fqdn: srv.target().to_string(),
                        })
                        .collect();
                    let _ = self.tx.broadcast(addrs);
                }
            }
            if *self.shutdown.borrow() {
                return;
            }
            tokio::time::delay_for(std::time::Duration::from_secs(self.discovery_dns_interval as u64)).await;
        }
    }
}
