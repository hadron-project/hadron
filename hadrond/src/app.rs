use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use tokio::stream::StreamExt;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tonic::transport::Channel;

use crate::config::Config;
use crate::core::{HCore, HCoreClientRequest, HCoreRequest};
use crate::discovery::Discovery;
use crate::network::{ClientRequest, Network, NetworkOutput, PeerRequest};
use crate::storage;

pub struct App {
    /// The ID of this node in the Raft cluster.
    node_id: u64,

    /// A channel used for sending control messages to the app layer.
    app_tx: mpsc::UnboundedSender<AppCtl>,
    /// A channel used for receiving control messages for the app layer.
    app_rx: mpsc::UnboundedReceiver<AppCtl>,

    /// A channel of data flowing in from the network layer.
    network_rx: mpsc::UnboundedReceiver<NetworkOutput>,
    /// A signal mapping peer nodes to their communication channels.
    peers_rx: watch::Receiver<Arc<HashMap<u64, Channel>>>,

    /// A channel used for sending requests into the Hadron core.
    hcore_tx: mpsc::UnboundedSender<HCoreRequest>,

    discovery: JoinHandle<()>,
    network: JoinHandle<()>,
    hcore: JoinHandle<()>,
}

impl App {
    pub async fn new(config: Arc<Config>) -> Result<Self> {
        let (app_tx, app_rx) = mpsc::unbounded_channel();

        // Fetch this node's ID from disk.
        let node_id = storage::get_node_id(&*config).await?;
        tracing::info!(node_id);

        // Spawn the discovery actor.
        let (discovery, discovery_rx) = Discovery::new(config.clone());
        let discovery = discovery.spawn();

        // Spawn the network actor.
        let (network, network_rx, peers_rx) = Network::new(node_id, config.clone(), discovery_rx)?;
        let network = network.spawn();

        // Spawn the Hadron core data layer.
        let (hcore_tx, hcore_rx) = mpsc::unbounded_channel();
        let hcore = HCore::new(node_id, config.clone(), peers_rx.clone(), hcore_rx).await?.spawn();

        // Setup signal handlers to handle graceful shutdown.
        // TODO: ^^^

        Ok(Self {
            node_id,
            app_tx,
            app_rx,
            network_rx,
            peers_rx,
            hcore_tx,
            discovery,
            network,
            hcore,
        })
    }

    pub fn spawn(self) -> JoinHandle<()> {
        tokio::spawn(self.run())
    }

    async fn run(mut self) {
        loop {
            tokio::select! {
                Some(app_ctl) = self.app_rx.next() => self.handle_app_ctl_msg(app_ctl).await,
                Some(net_req) = self.network_rx.next() => self.handle_network_request(net_req).await,
            }
        }
    }

    #[tracing::instrument(level = "trace", skip(self, msg))]
    async fn handle_app_ctl_msg(&mut self, msg: AppCtl) {
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
                ClientRequest::EphemeralPub(req) => todo!("finish this up"),
                ClientRequest::EphemeralSub(req) => todo!("finish this up"),
                ClientRequest::RpcPub(req) => todo!("finish this up"),
                ClientRequest::RpcSub(req) => todo!("finish this up"),
                ClientRequest::Transaction(req) => {
                    let _ = self.hcore_tx.send(HCoreRequest::Client(HCoreClientRequest::Transaction(req)));
                }
                ClientRequest::StreamPub(req) => {
                    let _ = self.hcore_tx.send(HCoreRequest::Client(HCoreClientRequest::StreamPub(req)));
                }
                ClientRequest::StreamSub(req) => {
                    let _ = self.hcore_tx.send(HCoreRequest::Client(HCoreClientRequest::StreamSub(req)));
                }
                ClientRequest::StreamUnsub(req) => {
                    let _ = self.hcore_tx.send(HCoreRequest::Client(HCoreClientRequest::StreamUnsub(req)));
                }
                ClientRequest::PipelineStageSub(req) => {
                    let _ = self.hcore_tx.send(HCoreRequest::Client(HCoreClientRequest::PipelineStageSub(req)));
                }
                ClientRequest::UpdateSchema(req) => {
                    let _ = self.hcore_tx.send(HCoreRequest::Client(HCoreClientRequest::UpdateSchema(req)));
                }
            },
        }
    }
}

/// Application control messages coming into the app from other components.
pub enum AppCtl {}
