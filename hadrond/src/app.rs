#![allow(unused_imports)] // TODO: remove this.
#![allow(unused_variables)] // TODO: remove this.
#![allow(unused_mut)] // TODO: remove this.
#![allow(dead_code)] // TODO: remove this.

use std::collections::{BTreeMap, HashMap};
use std::hash::Hasher;
use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::signal::unix::{signal, SignalKind};
use tokio::stream::{StreamExt, StreamMap};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tonic::transport::Channel;

use crate::config::Config;
use crate::ctl_placement::events::CPCEvent;
use crate::ctl_placement::Cpc;
use crate::ctl_raft::models::{CRCClientRequest, CRCRequest};
use crate::ctl_raft::{CRCIndex, CRC};
use crate::ctl_stream::{Spc, SpcInput};
use crate::database::Database;
use crate::discovery::Discovery;
use crate::models::{placement, prelude::*};
use crate::network::{ClientRequest, Network, NetworkOutput, PeerRequest, StreamPub};
use crate::NodeId;

pub struct App {
    /// The ID of this node in the Raft cluster.
    id: NodeId,
    /// The application's runtime config.
    config: Arc<Config>,
    /// The application's database system.
    db: Database,
    /// The system data index.
    index: Arc<CRCIndex>,

    /// A channel of data flowing in from the network layer.
    network_rx: mpsc::UnboundedReceiver<NetworkOutput>,
    /// A signal mapping peer nodes to their communication channels.
    peers_rx: watch::Receiver<Arc<HashMap<u64, Channel>>>,

    /// A channel used for sending requests into the Hadron core.
    crc_tx: mpsc::UnboundedSender<CRCRequest>,
    /// A channel of messages coming out of the CPC.
    cpc_rx: mpsc::UnboundedReceiver<CPCEvent>,

    /// A channel used for triggering graceful shutdown.
    shutdown_rx: watch::Receiver<bool>,
    /// A channel used for triggering graceful shutdown.
    shutdown_tx: watch::Sender<bool>,

    /// Channels and handles for all spawned stream partition controllers on this node.
    ///
    /// These are indexed by their CG ID, as only a single replica per control group may run on a
    /// node at any time. Moreover, routing of connections is handled by partition/CG ID.
    stream_control_groups: BTreeMap<u64, (mpsc::Sender<SpcInput>, JoinHandle<Result<()>>)>,

    discovery: JoinHandle<()>,
    network: JoinHandle<()>,
    crc: JoinHandle<()>,
    cpc: JoinHandle<()>,
}

impl App {
    pub async fn new(config: Arc<Config>) -> Result<Self> {
        // App shutdown channel.
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let (peers_tx, peers_rx) = watch::channel(Default::default());

        // Fetch this node's ID from disk.
        let db = Database::new(config.clone()).await.context("error opening database")?;
        let node_id = db.get_node_id().await.context("error getting node ID")?;
        tracing::info!(node_id);

        // Spawn the CRC.
        let (crc_tx, crc_rx) = mpsc::unbounded_channel();
        let (crc, index, crc_events, raft_metrics) =
            CRC::new(node_id, config.clone(), db.clone(), peers_rx.clone(), crc_rx, shutdown_rx.clone()).await?;
        let crc = crc.spawn();

        // Spawn the CPC.
        let (cpc, cpc_rx) = Cpc::new(node_id, config.clone(), shutdown_rx.clone(), raft_metrics, crc_events, crc_tx.clone());
        let cpc = cpc.spawn();

        // Spawn the discovery layer.
        let (discovery, discovery_rx) = Discovery::new(config.clone(), shutdown_rx.clone());
        let discovery = discovery.spawn();

        // Spawn the network layer.
        let (network, network_rx) = Network::new(node_id, config.clone(), index.clone(), discovery_rx, peers_tx, shutdown_rx.clone())?;
        let network = network.spawn();

        Ok(Self {
            id: node_id,
            config,
            db,
            index,
            network_rx,
            peers_rx,
            crc_tx,
            cpc_rx,
            shutdown_rx,
            shutdown_tx,
            stream_control_groups: Default::default(),
            discovery,
            network,
            crc,
            cpc,
        })
    }

    pub fn spawn(self) -> JoinHandle<Result<()>> {
        tokio::spawn(self.run())
    }

    async fn run(mut self) -> Result<()> {
        let mut signals = StreamMap::new();
        signals.insert("sigterm", signal(SignalKind::terminate()).context("error building signal stream")?);
        signals.insert("sigint", signal(SignalKind::interrupt()).context("error building signal stream")?);

        loop {
            tokio::select! {
                Some(net_req) = self.network_rx.next() => self.handle_network_request(net_req).await,
                Some(event) = self.cpc_rx.next() => self.handle_placement_event(event).await,
                Some((_, sig)) = signals.next() => {
                    tracing::debug!(signal = ?sig, "signal received, beginning graceful shutdown");
                    self.shutdown();
                    break;
                }
                Some(needs_shutdown) = self.shutdown_rx.next() => if needs_shutdown { break } else { continue },
            }
        }

        // Begin shutdown routine.
        tracing::debug!("Hadron is shutting down");
        if let Err(err) = self.discovery.await {
            tracing::error!(error = ?err);
        }
        if let Err(err) = self.network.await {
            tracing::error!(error = ?err);
        }
        if let Err(err) = self.crc.await {
            tracing::error!(error = ?err);
        }
        if let Err(err) = self.cpc.await {
            tracing::error!(error = ?err);
        }
        tracing::debug!("Hadron shutdown");
        Ok(())
    }

    /// Trigger a system shutdown.
    #[tracing::instrument(level = "trace", skip(self))]
    fn shutdown(&mut self) {
        let _ = self.shutdown_tx.broadcast(true);
    }

    /// Handle a placement event coming from the CPC.
    ///
    /// Each event indicates some pertinent scheduling event related to this node. This handle
    /// is used to spawn all other dynamic controllers for this node.
    #[tracing::instrument(level = "trace", skip(self, event))]
    async fn handle_placement_event(&mut self, event: CPCEvent) {
        match event {
            CPCEvent::StreamReplicaScheduled(replica) => self.spawn_stream_replica(replica).await,
            CPCEvent::PipelineReplicaScheduled(replica) => self.spawn_pipeline_replica(replica).await,
        }
    }

    /// Spawn a stream replica controller on this node.
    #[tracing::instrument(
        level = "trace",
        skip(self, replica),
        fields(replica = replica.id, cg = replica.cg_id, stream = replica.stream_id),
    )]
    async fn spawn_stream_replica(&mut self, replica: Arc<placement::StreamReplica>) {
        // Get a pointer to the corresponding stream object.
        let stream = match self.index.streams.get(&replica.stream_id).map(|res| res.value().clone()) {
            Some(stream) => stream,
            None => {
                tracing::error!("error spawning stream replica, stream model not found in index");
                self.shutdown();
                return;
            }
        };

        // Get a pointer to the corresponding control group of the replica.
        let cg = match self.index.control_groups.get(&replica.cg_id).map(|res| res.value().clone()) {
            Some(cg) => cg,
            None => {
                tracing::error!("error spawning stream replica, control group model not found in index");
                self.shutdown();
                return;
            }
        };

        // Spawn the stream partition controller for this replica.
        let (id, config, shutdown, peers) = (cg.id, self.config.clone(), self.shutdown_rx.clone(), self.peers_rx.clone());
        let (ns, name, partition, replica_id) = (stream.model.namespace(), stream.model.name(), replica.partition, replica.id);
        let (app_tx, app_rx) = mpsc::channel(1000);
        let handle_res = Spc::new(self.id, config, self.db.clone(), shutdown, peers, app_rx, replica, cg, stream.clone())
            .await
            .with_context(|| format!("error initializing SPC {}/{}/{}/{}", ns, name, partition, replica_id));
        let handle = match handle_res {
            Ok(handle) => handle.spawn(),
            Err(err) => {
                tracing::error!(error = ?err, "error spawning stream partition controller");
                self.shutdown();
                return;
            }
        };
        self.stream_control_groups.insert(id, (app_tx, handle));
    }

    /// Spawn a pipeline replica controller on this node.
    #[tracing::instrument(level = "trace", skip(self, replica))]
    async fn spawn_pipeline_replica(&mut self, replica: Arc<placement::PipelineReplica>) {
        // TODO: spawn a stream partition controller for the given replica.
    }

    /// Handle a network request.
    #[tracing::instrument(level = "trace", skip(self, req))]
    async fn handle_network_request(&mut self, req: NetworkOutput) {
        match req {
            NetworkOutput::PeerRequest(peer_req) => match peer_req {
                PeerRequest::RaftAppendEntries(req) => {
                    let _ = self.crc_tx.send(CRCRequest::RaftAppendEntries(req));
                }
                PeerRequest::RaftInstallSnapshot(req) => {
                    let _ = self.crc_tx.send(CRCRequest::RaftInstallSnapshot(req));
                }
                PeerRequest::RaftVote(req) => {
                    let _ = self.crc_tx.send(CRCRequest::RaftVote(req));
                }
            },
            NetworkOutput::ClientRequest(client_req) => match client_req {
                ClientRequest::EphemeralPub(_req) => todo!("finish this up"),
                ClientRequest::EphemeralSub(_req) => todo!("finish this up"),
                ClientRequest::RpcPub(_req) => todo!("finish this up"),
                ClientRequest::RpcSub(_req) => todo!("finish this up"),
                ClientRequest::Transaction(_req) => todo!("finish this up"),
                ClientRequest::StreamPub(req) => {
                    let tx = match self.stream_control_groups.get_mut(&req.cg.id) {
                        Some((tx, _)) => tx,
                        None => {
                            let _ = req.tx.send(Err(tonic::Status::unavailable("stream controller not yet available")));
                            return;
                        }
                    };
                    let _ = tx.send(SpcInput::StreamPub(req));
                }
                ClientRequest::StreamSub(_req) => todo!("finish this up"),
                ClientRequest::StreamUnsub(_req) => todo!("finish this up"),
                ClientRequest::PipelineStageSub(_req) => todo!("finish this up"),

                // Requests bound for the Cluster Raft Controller.
                ClientRequest::UpdateSchema(req) => {
                    let _ = self.crc_tx.send(CRCRequest::Client(CRCClientRequest::UpdateSchema(req)));
                }
            },
        }
    }
}
