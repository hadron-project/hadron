use std::sync::Arc;

use anyhow::Result;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::config::Config;
use crate::discovery::Discovery;
use crate::network::Network;
use crate::storage;

pub struct App {
    /// The ID of this node in the Raft cluster.
    node_id: u64,
    discovery: JoinHandle<()>,
    network: JoinHandle<()>,
}

impl App {
    pub async fn new(config: Arc<Config>) -> Result<Self> {
        // Fetch this node's ID from disk.
        let node_id = storage::get_node_id(&*config).await?;
        tracing::info!(node_id);

        // Spawn the discovery actor.
        let (discovery_tx, discovery_rx) = mpsc::channel(1);
        let discovery = Discovery::new(config.clone(), discovery_tx).spawn();

        // Spawn the network actor.
        let network = Network::new(node_id, config.clone(), discovery_rx)?.spawn();

        Ok(Self { node_id, discovery, network })
    }

    pub fn spawn(self) -> JoinHandle<()> {
        tokio::spawn(self.run())
    }

    async fn run(self) {
        let _ = tokio::signal::ctrl_c().await;
        tracing::info!("app is closing");
    }
}
