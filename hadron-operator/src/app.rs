use std::sync::Arc;

use anyhow::{Context, Result};
use futures::stream::StreamExt;
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tokio_stream::wrappers::{BroadcastStream, SignalStream};
use tokio_stream::StreamMap;

use crate::config::Config;
use crate::k8s::Controller;
use crate::server::AppServer;

/// The application object for when Hadron is running as a server.
pub struct App {
    /// The application's runtime config.
    _config: Arc<Config>,

    /// A channel used for triggering graceful shutdown.
    shutdown_tx: broadcast::Sender<()>,
    /// A channel used for triggering graceful shutdown.
    shutdown_rx: BroadcastStream<()>,

    /// The join handle of the gRPC server.
    server: JoinHandle<()>,
    /// The join handle of the K8s controller.
    controller: JoinHandle<Result<()>>,
}

impl App {
    /// Create a new instance.
    pub async fn new(config: Arc<Config>) -> Result<Self> {
        // App shutdown channel.
        let (shutdown_tx, shutdown_rx) = broadcast::channel(10);

        // Initialize K8s client.
        let client = kube::Client::try_default().await.context("error initializing K8s client")?;

        // Spawn various core tasks.
        let server = AppServer::new(config.clone(), shutdown_tx.clone()).spawn().await.context("error setting up client gRPC server")?;

        let controller = Controller::new(client, config.clone(), shutdown_tx.clone())?.spawn();

        Ok(Self {
            _config: config,
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
        signals.insert("sigterm", SignalStream::new(signal(SignalKind::terminate()).context("error building signal stream")?));
        signals.insert("sigint", SignalStream::new(signal(SignalKind::interrupt()).context("error building signal stream")?));

        loop {
            tokio::select! {
                Some((_, sig)) = signals.next() => {
                    tracing::debug!(signal = ?sig, "signal received, beginning graceful shutdown");
                    let _ = self.shutdown_tx.send(());
                    break;
                }
                _ = self.shutdown_rx.next() => break,
            }
        }

        // Begin shutdown routine.
        tracing::debug!("Hadron Operator is shutting down");
        if let Err(err) = self.server.await {
            tracing::error!(error = ?err, "error joining client gRPC server task");
        }
        if let Err(err) = self.controller.await.context("error joining k8s controller handle").and_then(|res| res) {
            tracing::error!(error = ?err, "error shutting down k8s controller");
        }

        tracing::debug!("Hadron Operator shutdown complete");
        Ok(())
    }
}
