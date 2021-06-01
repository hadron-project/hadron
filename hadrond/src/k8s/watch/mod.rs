//! Kubernetes controller for watching Hadron CRs.

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use futures::prelude::*;
use kube::api::{Api, ListParams};
use kube::client::Client;
use kube::Error as KubeError;
use kube_runtime::watcher::{watcher, Error as WatcherError, Event};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tokio_stream::wrappers::BroadcastStream;

use crate::crd::stream::Stream;

type StreamEventResult = std::result::Result<Event<Stream>, WatcherError>;

/// Kubernetes controller for watching Hadron CRs.
pub struct Controller {
    /// K8s client.
    client: Client,
    /// A channel used for triggering graceful shutdown.
    shutdown_tx: broadcast::Sender<()>,
    /// A channel used for triggering graceful shutdown.
    shutdown_rx: BroadcastStream<()>,
}

impl Controller {
    /// Create a new instance.
    pub fn new(client: Client, shutdown_tx: broadcast::Sender<()>) -> Self {
        Self {
            client,
            shutdown_rx: BroadcastStream::new(shutdown_tx.subscribe()),
            shutdown_tx,
        }
    }

    pub fn spawn(self) -> JoinHandle<Result<()>> {
        tokio::spawn(self.run())
    }

    async fn run(mut self) -> Result<()> {
        let streams: Api<Stream> = Api::all(self.client.clone());
        let mut streams_watcher = watcher(streams, ListParams::default()).boxed();

        tracing::info!("k8s watcher initialized");
        loop {
            tokio::select! {
                Some(k8s_event_res) = streams_watcher.next() => self.handle_stream_event(k8s_event_res).await,
                Some(_) = self.shutdown_rx.next() => break,
            }
        }

        tracing::debug!("k8s watcher shutdown");
        Ok(())
    }

    /// Handle `Stream` watcher event.
    #[tracing::instrument(level = "debug", skip(self, res))]
    async fn handle_stream_event(&mut self, res: StreamEventResult) {
        let event = match res {
            Ok(event) => event,
            Err(err) => {
                tracing::error!(error = ?err, "error from Stream k8s watcher");
                let _ = tokio::time::sleep(Duration::from_secs(10)).await;
                return;
            }
        };
        match event {
            // A resource was added or modified.
            Event::Applied(ingress) => todo!(),
            // A resource was deleted.
            Event::Deleted(ingress) => todo!(),
            // The watch stream was restarted. Should be used as a signal
            // to replace the store contents atomically.
            Event::Restarted(all_ingress) => todo!(),
        }
    }

    // /// Handle K8s events where the given ingress object was created or updated.
    // #[tracing::instrument(level = "debug", skip(self, ingress))]
    // async fn handle_applied_pgingress(&mut self, ingress: models::PGIngressPrivateLink) {
    //     // Extract needed info for storing the object.
    //     let name = ingress.metadata.name.as_deref().unwrap_or("");
    //     let namespace = ingress.metadata.namespace.as_deref().unwrap_or("");
    //     let id = format!("{}/{}", name, namespace);

    //     // Check to see if we have an older copy of the object.
    //     match self.ingress.get(&id) {
    //         // If so and the rules are the same, then store the new copy and we are done.
    //         Some(old) if old.spec.rules == ingress.spec.rules => {
    //             tracing::debug!("ingress record rules are unchanged, done");
    //             self.ingress.insert(id, ingress);
    //             return;
    //         }
    //         // Else if the rules are different, then we need to update our routing table rules.
    //         // Here we will just remove the old routes, which will be replaced later.
    //         Some(old) => {
    //             tracing::debug!("ingress record has changed, pruning old rules");
    //             for rule in old.spec.rules.iter() {
    //                 self.rtable.remove_route(Rule::from(rule)).await;
    //                 self.metrics.ingress_rules.dec();
    //             }
    //         }
    //         // Else, the ingress did not exist before, so add it to our set of known ingresses.
    //         _ => {
    //             tracing::debug!("inserted new ingress record");
    //             self.ingress.insert(id, ingress.clone());
    //         }
    //     }

    //     // If we are here, then we need to update our routing table with the ingress rules.
    //     for rule in ingress.spec.rules.iter() {
    //         self.rtable.insert_route(Rule::from(rule), rule.backend.clone()).await;
    //         self.metrics.ingress_rules.inc();
    //         tracing::debug!("ingress rule added");
    //     }
    // }

    // /// Handle K8s events where the given ingress object was deleted.
    // #[tracing::instrument(level = "debug", skip(self, ingress))]
    // async fn handle_deleted_pgingress(&mut self, ingress: models::PGIngressPrivateLink) {
    //     tracing::debug!("handling deleted PGIngress");
    //     // Extract needed info for storing the object.
    //     let name = ingress.metadata.name.as_deref().unwrap_or("");
    //     let namespace = ingress.metadata.namespace.as_deref().unwrap_or("");
    //     let id = format!("{}/{}", name, namespace);

    //     // Remove ingress object mapping.
    //     if self.ingress.remove(&id).is_none() {
    //         tracing::debug!("ingress record delete which did not exist in records map, skipping");
    //         return; // Nothing to do.
    //     }

    //     // Delete any outstanding rules in the routing table for this ingress.
    //     for rule in ingress.spec.rules.iter() {
    //         self.rtable.remove_route(Rule::from(rule)).await;
    //         self.metrics.ingress_rules.dec();
    //         tracing::debug!("ingress rule deleted");
    //     }
    // }

    // /// Handle K8s events where the watcher stream for all PGIngress objects was restarted.
    // ///
    // /// When this happens, we build a new ID map & a new routing table rule map. Once they have
    // /// been fully built, we swap their contents in quick succession.
    // #[tracing::instrument(level = "debug", skip(self, all_ingress))]
    // async fn handle_restarted_stream_pgingress(&mut self, all_ingress: Vec<models::PGIngressPrivateLink>) {
    //     let new_rtable = RoutingTable::default();
    //     let mut id_map = HashMap::new();

    //     for ingress in all_ingress {
    //         // Add routing table components.
    //         for rule in ingress.spec.rules.iter() {
    //             new_rtable.insert_route(Rule::from(rule), rule.backend.clone()).await;
    //         }

    //         // Extract needed info for storing the object.
    //         let name = ingress.metadata.name.as_deref().unwrap_or("");
    //         let namespace = ingress.metadata.namespace.as_deref().unwrap_or("");
    //         let id = format!("{}/{}", name, namespace);
    //         id_map.insert(id, ingress);
    //     }

    //     // Swap both mappings in quick succession.
    //     let new_rules_count = new_rtable.read().await.len();
    //     std::mem::swap(&mut *self.rtable.write().await, &mut *new_rtable.write().await);
    //     std::mem::swap(&mut self.ingress, &mut id_map);

    //     // Update metrics based on updates.
    //     self.metrics.ingress_rules.set(new_rules_count as i64);
    //     tracing::debug!("PGIngress stream restarted successfully");
    // }
}
