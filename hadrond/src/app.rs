use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use tokio::signal::unix::{signal, SignalKind};
use tokio::stream::{StreamExt, StreamMap};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tonic::transport::Channel;

use crate::config::Config;
use crate::crc::{HCoreClientRequest, HCoreRequest, CRC};
use crate::discovery::Discovery;
use crate::network::{ClientRequest, Network, NetworkOutput, PeerRequest};
use crate::storage;

pub struct App {
    /// The ID of this node in the Raft cluster.
    _node_id: u64,

    /// A channel used for sending control messages to the app layer.
    _app_tx: mpsc::UnboundedSender<AppCtl>,
    /// A channel used for receiving control messages for the app layer.
    app_rx: mpsc::UnboundedReceiver<AppCtl>,

    /// A channel of data flowing in from the network layer.
    network_rx: mpsc::UnboundedReceiver<NetworkOutput>,
    /// A signal mapping peer nodes to their communication channels.
    _peers_rx: watch::Receiver<Arc<HashMap<u64, Channel>>>,

    /// A channel used for sending requests into the Hadron core.
    hcore_tx: mpsc::UnboundedSender<HCoreRequest>,

    /// A channel used for triggering graceful shutdown.
    shutdown_tx: watch::Sender<bool>,

    discovery: JoinHandle<()>,
    network: JoinHandle<()>,
    crc: JoinHandle<()>,
}

impl App {
    pub async fn new(config: Arc<Config>) -> Result<Self> {
        // App shutdown channel.
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        // App control channel.
        let (app_tx, app_rx) = mpsc::unbounded_channel();

        // Fetch this node's ID from disk.
        let node_id = storage::get_node_id(&*config).await?;
        tracing::info!(node_id);

        // Spawn the discovery actor.
        let (discovery, discovery_rx) = Discovery::new(config.clone(), shutdown_rx.clone());
        let discovery = discovery.spawn();

        // Spawn the network actor.
        let (network, network_rx, peers_rx) = Network::new(node_id, config.clone(), discovery_rx, shutdown_rx.clone())?;
        let network = network.spawn();

        // Spawn the Hadron core data layer.
        let (hcore_tx, hcore_rx) = mpsc::unbounded_channel();
        let crc = CRC::new(node_id, config.clone(), peers_rx.clone(), hcore_rx, shutdown_rx).await?.spawn();

        Ok(Self {
            _node_id: node_id,
            _app_tx: app_tx,
            app_rx,
            network_rx,
            _peers_rx: peers_rx,
            hcore_tx,
            shutdown_tx,
            discovery,
            network,
            crc,
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
                Some(app_ctl) = self.app_rx.next() => self.handle_app_ctl_msg(app_ctl).await,
                Some(net_req) = self.network_rx.next() => self.handle_network_request(net_req).await,
                Some((_, sig)) = signals.next() => {
                    tracing::warn!(signal = ?sig, "signal received, beginning graceful shutdown");
                    let _ = self.shutdown_tx.broadcast(true);
                    break;
                }
            }
        }

        // Begin shutdown routine.
        if let Err(err) = self.discovery.await {
            tracing::error!(error = ?err);
        }
        if let Err(err) = self.network.await {
            tracing::error!(error = ?err);
        }
        if let Err(err) = self.crc.await {
            tracing::error!(error = ?err);
        }
        Ok(())
    }

    #[tracing::instrument(level = "trace", skip(self, _msg))]
    async fn handle_app_ctl_msg(&mut self, _msg: AppCtl) {
        todo!()
    }

    #[tracing::instrument(level = "trace", skip(self, req))]
    async fn handle_network_request(&mut self, req: NetworkOutput) {
        match req {
            NetworkOutput::PeerRequest(peer_req) => match peer_req {
                PeerRequest::RaftAppendEntries(req) => {
                    let _ = self.hcore_tx.send(HCoreRequest::RaftAppendEntries(req));
                }
                PeerRequest::RaftInstallSnapshot(req) => {
                    let _ = self.hcore_tx.send(HCoreRequest::RaftInstallSnapshot(req));
                }
                PeerRequest::RaftVote(req) => {
                    let _ = self.hcore_tx.send(HCoreRequest::RaftVote(req));
                }
            },
            NetworkOutput::ClientRequest(client_req) => match client_req {
                ClientRequest::EphemeralPub(_req) => todo!("finish this up"),
                ClientRequest::EphemeralSub(_req) => todo!("finish this up"),
                ClientRequest::RpcPub(_req) => todo!("finish this up"),
                ClientRequest::RpcSub(_req) => todo!("finish this up"),
                ClientRequest::Transaction(req) => todo!("finish this up"),
                ClientRequest::StreamPub(req) => todo!("finish this up"),
                ClientRequest::StreamSub(req) => todo!("finish this up"),
                ClientRequest::StreamUnsub(req) => todo!("finish this up"),
                ClientRequest::PipelineStageSub(req) => todo!("finish this up"),

                // Requests bound for the Cluster Raft Controller.
                // SOON: Hadron cluster membership.
                // SOON: Leadership designations for other controllers within the Hadron cluster.
                // SOON: AuthN & authZ resources such as users and tokens.
                ClientRequest::UpdateSchema(req) => {
                    let _ = self.hcore_tx.send(HCoreRequest::Client(HCoreClientRequest::UpdateSchema(req)));
                }
            },
        }
    }
}

/// Application control messages coming into the app from other components.
pub enum AppCtl {}
