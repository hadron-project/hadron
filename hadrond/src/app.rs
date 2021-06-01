use std::sync::Arc;

use anyhow::{Context, Result};
use futures::stream::StreamExt;
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tokio_stream::wrappers::{BroadcastStream, SignalStream};
use tokio_stream::StreamMap;

use crate::config::Config;
use crate::database::Database;
use crate::k8s::watch::Controller;
use crate::server::Server;

pub struct AppServer {
    /// The application's runtime config.
    config: Arc<Config>,
    /// The application's database system.
    db: Database,

    /// A channel used for triggering graceful shutdown.
    shutdown_tx: broadcast::Sender<()>,
    /// A channel used for triggering graceful shutdown.
    shutdown_rx: BroadcastStream<()>,
    /// A handle to the network server.
    server: JoinHandle<Result<()>>,
    /// A handle to the k8s watcher controller.
    controller: JoinHandle<Result<()>>,
}

impl AppServer {
    /// Create a new instance.
    pub async fn new(config: Arc<Config>) -> Result<Self> {
        // App shutdown channel.
        let (shutdown_tx, shutdown_rx) = broadcast::channel(100);
        let (events_tx, events_rx) = broadcast::channel(10_000); // 10k before lagging.

        // Initialize this node's storage.
        let db = Database::new(config.clone()).await.context("error opening database")?;

        // Spawn the network server.
        let (server, _cache) = Server::new(config.clone(), db.clone(), shutdown_tx.clone(), events_tx.clone(), events_rx)
            .await
            .context("error creating network server")?;
        let server = server.spawn();

        // Initialize the K8s client.
        let client = kube::Client::try_default().await.context("error initializing K8s client")?;
        let controller = Controller::new(client, shutdown_tx.clone()).spawn();

        Ok(Self {
            config,
            db,
            shutdown_rx: BroadcastStream::new(shutdown_rx),
            shutdown_tx,
            server,
            controller,
        })
    }

    pub fn spawn(self) -> JoinHandle<Result<()>> {
        tokio::spawn(self.run())
    }

    async fn run(mut self) -> Result<()> {
        let mut signals = StreamMap::new();
        signals.insert(
            "sigterm",
            SignalStream::new(signal(SignalKind::terminate()).context("error building signal stream")?),
        );
        signals.insert(
            "sigint",
            SignalStream::new(signal(SignalKind::interrupt()).context("error building signal stream")?),
        );

        loop {
            tokio::select! {
                Some((_, sig)) = signals.next() => {
                    tracing::debug!(signal = ?sig, "signal received, beginning graceful shutdown");
                    self.shutdown();
                    break;
                }
                Some(_) = self.shutdown_rx.next() => break,
            }
        }

        // Begin shutdown routine.
        tracing::debug!("Hadron is shutting down");
        if let Err(err) = self.server.await {
            tracing::error!(error = ?err, "error shutting down network server");
        }
        if let Err(err) = self.controller.await {
            tracing::error!(error = ?err, "error shutting down k8s watcher controller");
        }
        tracing::debug!("Hadron shutdown complete");
        Ok(())
    }

    /// Trigger a system shutdown.
    #[tracing::instrument(level = "trace", skip(self))]
    fn shutdown(&mut self) {
        let _ = self.shutdown_tx.send(());
    }
}
