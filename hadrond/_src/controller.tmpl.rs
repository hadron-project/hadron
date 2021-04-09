//! Network server.

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
use crate::NodeId;

/// Network server used to handle client requests.
pub struct Server {
    /// The application's runtime config.
    config: Arc<Config>,
    /// The application's database system.
    db: Database,
    /// A channel used for triggering graceful shutdown.
    shutdown_rx: WatchStream<bool>,
}

impl Server {
    /// Create a new instance.
    pub async fn new(config: Arc<Config>, db: Database, shutdown_rx: WatchStream<bool>) -> Result<Self> {
        Ok(Self { config, db, shutdown_rx })
    }

    pub fn spawn(self) -> JoinHandle<Result<()>> {
        tokio::spawn(self.run())
    }

    async fn run(mut self) -> Result<()> {
        loop {
            tokio::select! {
                Some(needs_shutdown) = self.shutdown_rx.next() => if needs_shutdown { break } else { continue },
            }
        }

        // Begin shutdown routine.
        tracing::debug!("network server shutdown");
        Ok(())
    }
}
