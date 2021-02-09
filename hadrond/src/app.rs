#![allow(unused_imports)] // TODO: remove this.
#![allow(unused_variables)] // TODO: remove this.
#![allow(unused_mut)] // TODO: remove this.
#![allow(dead_code)] // TODO: remove this.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::signal::unix::{signal, SignalKind};
use tokio::stream::{StreamExt, StreamMap};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tonic::transport::Channel;

use crate::config::Config;
use crate::ctl_placement::CPC;
use crate::ctl_raft::{CRCClientRequest, CRCRequest, HCoreIndex, CRC};
use crate::database::Database;
use crate::discovery::Discovery;
use crate::network::{ClientRequest, Network, NetworkOutput, PeerRequest};

pub struct App {
    /// The ID of this node in the Raft cluster.
    _node_id: u64,
    /// The system data index.
    index: Arc<HCoreIndex>,

    /// A channel of data flowing in from the network layer.
    network_rx: mpsc::UnboundedReceiver<NetworkOutput>,
    /// A signal mapping peer nodes to their communication channels.
    _peers_rx: watch::Receiver<Arc<HashMap<u64, Channel>>>,

    /// A channel used for sending requests into the Hadron core.
    crc_tx: mpsc::UnboundedSender<CRCRequest>,

    /// A channel used for triggering graceful shutdown.
    shutdown_tx: watch::Sender<bool>,

    discovery: JoinHandle<()>,
    network: JoinHandle<()>,
    crc: JoinHandle<()>,
    cpc: JoinHandle<()>,
}

impl App {
    pub async fn new(config: Arc<Config>) -> Result<Self> {
        // App shutdown channel.
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        // Fetch this node's ID from disk.
        let db = Database::new(config.clone()).await.context("error opening database")?;
        let node_id = db.get_node_id().await.context("error getting node ID")?;
        tracing::info!(node_id);

        // Spawn the discovery layer.
        let (discovery, discovery_rx) = Discovery::new(config.clone(), shutdown_rx.clone());
        let discovery = discovery.spawn();

        // Spawn the network layer.
        let (network, network_rx, peers_rx) = Network::new(node_id, config.clone(), discovery_rx, shutdown_rx.clone())?;
        let network = network.spawn();

        // Spawn the CRC.
        let (crc_tx, crc_rx) = mpsc::unbounded_channel();
        let (crc, index, crc_events, raft_metrics) =
            CRC::new(node_id, config.clone(), db.clone(), peers_rx.clone(), crc_rx, shutdown_rx.clone()).await?;
        let crc = crc.spawn();

        // Spawn the CPC.
        let cpc = CPC::new(node_id, config.clone(), shutdown_rx, raft_metrics, crc_events).spawn();

        Ok(Self {
            _node_id: node_id,
            index,
            network_rx,
            _peers_rx: peers_rx,
            crc_tx,
            shutdown_tx,
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
                Some((_, sig)) = signals.next() => {
                    tracing::debug!(signal = ?sig, "signal received, beginning graceful shutdown");
                    let _ = self.shutdown_tx.broadcast(true);
                    break;
                }
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
                ClientRequest::StreamPub(_req) => todo!("finish this up"),
                ClientRequest::StreamSub(_req) => todo!("finish this up"),
                ClientRequest::StreamUnsub(_req) => todo!("finish this up"),
                ClientRequest::PipelineStageSub(_req) => todo!("finish this up"),

                // Requests bound for the Cluster Raft Controller.
                // SOON: Hadron cluster membership.
                // SOON: Leadership designations for other controllers within the Hadron cluster.
                // SOON: AuthN & authZ resources such as users and tokens.
                ClientRequest::UpdateSchema(req) => {
                    let _ = self.crc_tx.send(CRCRequest::Client(CRCClientRequest::UpdateSchema(req)));
                }
            },
        }
    }
}
