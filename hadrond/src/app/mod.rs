mod events;
mod network;
mod tasks;

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use futures::stream::StreamExt;
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tokio_stream::wrappers::{BroadcastStream, SignalStream};
use tokio_stream::StreamMap;
use tonic::transport::Server;

pub use crate::app::tasks::TokensMap;
use crate::config::Config;
use crate::database::Database;
use crate::grpc;
use crate::pipeline::PipelineHandle;

const CLIENT_ADDR: &str = "0.0.0.0:7000";

/// The application object for when Hadron is running as a server.
pub struct App {
    /// The application's runtime config.
    config: Arc<Config>,
    /// The application's database system.
    db: Database,

    /// A map active pipeline controllers to their communcation channels.
    pipelines: HashMap<String, PipelineHandle>,
    /// A map of all known Token CRs in the namespace.
    tokens: TokensMap,

    /// A channel used for triggering graceful shutdown.
    shutdown_tx: broadcast::Sender<()>,
    /// A channel used for triggering graceful shutdown.
    shutdown_rx: BroadcastStream<()>,
}

impl App {
    /// Create a new instance.
    pub async fn new(config: Arc<Config>) -> Result<Self> {
        // App shutdown channel.
        let (shutdown_tx, shutdown_rx) = broadcast::channel(10);

        // Initialize this node's storage.
        let db = Database::new(config.clone()).await.context("error opening database")?;

        // Spawn various core tasks.
        let tokens = tasks::watch_tokens();
        // let stream_ctl = TODO: finis this.

        let client_server =
            Self::setup_client_grpc_server(config.clone(), tokens.clone(), shutdown_tx.clone()).context("error setting up client gRPC server")?;

        Ok(Self {
            config,
            db,
            pipelines: Default::default(),
            tokens,
            shutdown_rx: BroadcastStream::new(shutdown_rx),
            shutdown_tx,
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
                _ = self.shutdown_rx.next() => break,
            }
        }

        // Begin shutdown routine.
        tracing::debug!("Hadron is shutting down");

        tracing::debug!("Hadron shutdown complete");
        Ok(())
    }

    /// Trigger a system shutdown.
    #[tracing::instrument(level = "trace", skip(self))]
    fn shutdown(&mut self) {
        let _ = self.shutdown_tx.send(());
    }

    /// Setup the client gRPC server.
    fn setup_client_grpc_server(config: Arc<Config>, tokens: TokensMap, shutdown: broadcast::Sender<()>) -> Result<JoinHandle<()>> {
        let addr = CLIENT_ADDR.parse().context("failed to parse listener address")?;
        let inner_service = network::AppNetwork::new(config, tokens);
        let service = grpc::StreamControllerServer::new(inner_service);
        let shutdown_rx = shutdown.subscribe();
        let fut = Server::builder()
            .add_service(service)
            .serve_with_shutdown(addr, async move {
                let _res = shutdown_rx.recv().await;
            });
        Ok(tokio::spawn(async move {
            if let Err(err) = fut.await {
                tracing::error!(error = ?err, "error from client gRPC server");
            }
            let _res = shutdown.send(());
        }))
    }
}
