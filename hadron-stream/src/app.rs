use std::sync::Arc;

use anyhow::{Context, Result};
use futures::stream::StreamExt;
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinHandle;
use tokio_stream::wrappers::{BroadcastStream, SignalStream};
use tokio_stream::StreamMap;

use crate::config::Config;
use crate::database::Database;
use crate::server::AppServer;
use crate::stream::StreamCtl;
use crate::watchers::{PipelineWatcher, PipelinesMap, SecretsMap, StreamWatcher, TokensMap, TokensWatcher};

/// The application object for when Hadron is running as a server.
pub struct App {
    /// The application's runtime config.
    _config: Arc<Config>,
    /// The application's database system.
    _db: Database,

    /// A map of active pipeline controllers to their communcation channels.
    _pipelines: PipelinesMap,
    /// A map of all known Token CRs in the namespace.
    _tokens: TokensMap,
    /// A map of all known Secrets in the namespace belonging to Hadron.
    _secrets: SecretsMap,

    /// A channel used for triggering graceful shutdown.
    shutdown_tx: broadcast::Sender<()>,
    /// A channel used for triggering graceful shutdown.
    shutdown_rx: BroadcastStream<()>,

    /// The join handle of the stream controller.
    stream_handle: JoinHandle<Result<()>>,
    /// The join handle of the stream CR watcher.
    stream_watcher_handle: JoinHandle<Result<()>>,
    /// The join handle of the tokens CR watcher.
    tokens_handle: JoinHandle<Result<()>>,
    /// The join handle of the pipelines CR watcher.
    pipelines_handle: JoinHandle<Result<()>>,
    /// The join handle of the client gRPC server.
    client_server: JoinHandle<()>,
}

impl App {
    /// Create a new instance.
    pub async fn new(config: Arc<Config>) -> Result<Self> {
        // App shutdown channel.
        let (shutdown_tx, shutdown_rx) = broadcast::channel(10);

        // Initialize this node's storage.
        let db = Database::new(config.clone()).await.context("error opening database")?;

        // Initialize K8s client.
        let client = kube::Client::try_default()
            .await
            .context("error initializing K8s client")?;

        // Spawn various core tasks.
        let (tokens, tokens_map, secrets_map) = TokensWatcher::new(client.clone(), config.clone(), shutdown_tx.subscribe());
        let tokens_handle = tokens.spawn();

        let (stream_tx, stream_rx) = mpsc::channel(1000);
        let (stream_ctl, stream_offset_signal) = StreamCtl::new(config.clone(), db.clone(), shutdown_tx.clone(), stream_tx.clone(), stream_rx)
            .await
            .context("error spawning stream controller")?;
        let stream_handle = stream_ctl.spawn();

        let (stream_watcher, metadata_rx) = StreamWatcher::new(client.clone(), config.clone(), shutdown_tx.subscribe());
        let stream_watcher_handle = stream_watcher.spawn();

        let (pipelines, pipelines_map) = PipelineWatcher::new(client, config.clone(), db.clone(), stream_offset_signal, shutdown_tx.clone());
        let pipelines_handle = pipelines.spawn();

        let client_server = AppServer::new(
            config.clone(),
            pipelines_map.clone(),
            tokens_map.clone(),
            secrets_map.clone(),
            metadata_rx,
            shutdown_tx.clone(),
            stream_tx,
        )
        .spawn()
        .context("error setting up client gRPC server")?;

        Ok(Self {
            _config: config,
            _db: db,
            _tokens: tokens_map,
            _secrets: secrets_map,
            _pipelines: pipelines_map,
            shutdown_rx: BroadcastStream::new(shutdown_rx),
            shutdown_tx,
            stream_handle,
            stream_watcher_handle,
            tokens_handle,
            pipelines_handle,
            client_server,
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
                    let _ = self.shutdown_tx.send(());
                    break;
                }
                _ = self.shutdown_rx.next() => break,
            }
        }

        // Begin shutdown routine.
        tracing::debug!("Hadron is shutting down");
        if let Err(err) = self
            .tokens_handle
            .await
            .context("error joining token watcher handle")
            .and_then(|res| res)
        {
            tracing::error!(error = ?err, "error shutting down tokens watcher");
        }
        if let Err(err) = self
            .pipelines_handle
            .await
            .context("error joining pipelines watcher handle")
            .and_then(|res| res)
        {
            tracing::error!(error = ?err, "error shutting down pipelines watcher");
        }
        if let Err(err) = self
            .stream_handle
            .await
            .context("error joining stream controller handle")
            .and_then(|res| res)
        {
            tracing::error!(error = ?err, "error shutting down stream controller");
        }
        if let Err(err) = self
            .stream_watcher_handle
            .await
            .context("error joining stream CR watcher handle")
            .and_then(|res| res)
        {
            tracing::error!(error = ?err, "error shutting down stream CR watcher");
        }
        if let Err(err) = self.client_server.await {
            tracing::error!(error = ?err, "error joining client gRPC server task");
        }

        tracing::debug!("Hadron shutdown complete");
        Ok(())
    }
}
