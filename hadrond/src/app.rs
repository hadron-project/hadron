use std::collections::{BTreeMap, HashMap};
use std::hash::Hasher;
use std::sync::Arc;

use anyhow::{Context, Result};
use futures::stream::StreamExt;
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tokio_stream::wrappers::{SignalStream, WatchStream};
use tokio_stream::StreamMap;

use crate::config::Config;
use crate::database::Database;
use crate::server::Server;
use crate::NodeId;

pub struct App {
    /// The application's runtime config.
    config: Arc<Config>,
    /// The application's database system.
    db: Database,

    /// A channel used for triggering graceful shutdown.
    shutdown_rx: WatchStream<bool>,
    /// A channel used for triggering graceful shutdown.
    shutdown_tx: watch::Sender<bool>,
}

impl App {
    /// Create a new instance.
    pub async fn new(config: Arc<Config>) -> Result<Self> {
        // App shutdown channel.
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        // Initialize this nodes storage.
        let db = Database::new(config.clone()).await.context("error opening database")?;

        // Spawn the network server.
        let server = Server::new(config.clone(), db.clone(), shutdown_rx.clone())
            .await
            .context("error creating network server")?;
        let server = server.spawn();

        Ok(Self {
            config,
            db,
            shutdown_rx: WatchStream::new(shutdown_rx),
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
                Some(needs_shutdown) = self.shutdown_rx.next() => if needs_shutdown { break } else { continue },
            }
        }

        // Begin shutdown routine.
        tracing::debug!("Hadron is shutting down");
        tracing::debug!("Hadron shutdown");
        Ok(())
    }

    /// Trigger a system shutdown.
    #[tracing::instrument(level = "trace", skip(self))]
    fn shutdown(&mut self) {
        let _ = self.shutdown_tx.send(true);
    }
}
