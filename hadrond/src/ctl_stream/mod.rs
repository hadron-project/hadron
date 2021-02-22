//! Stream Partition Controller.
//!
//! This controller is responsible for a partition of a stream. All reads & writes for any specific
//! partition will be handled by the leader of a SPC group. Groups are composed of the replicas
//! of a partition.

use std::sync::Arc;

use anyhow::Result;
use tokio::stream::StreamExt;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::config::Config;
use crate::database::Database;
use crate::models::placement::StreamReplica;
use crate::NodeId;

/// The Stream Partition Controller (SPC).
pub struct SPC {
    /// The ID of this node in the Hadron cluster.
    node_id: NodeId,
    /// The application's runtime config.
    config: Arc<Config>,
    /// Application shutdown channel.
    shutdown: watch::Receiver<bool>,

    /// The object model of this stream replica.
    replica: Arc<StreamReplica>,
}

impl SPC {
    pub fn new(node_id: NodeId, config: Arc<Config>, shutdown: watch::Receiver<bool>, replica: Arc<StreamReplica>) -> Self {
        Self {
            node_id,
            config,
            shutdown,
            replica,
        }
    }

    pub fn spawn(self) -> JoinHandle<Result<()>> {
        tokio::spawn(self.run())
    }

    async fn run(mut self) -> Result<()> {
        tracing::debug!(
            id = self.replica.id,
            "SPC started for {}/{}/{}/{}",
            self.replica.namespace,
            self.replica.name,
            self.replica.partition,
            self.replica.replica
        );

        loop {
            tokio::select! {
                Some(needs_shutdown) = self.shutdown.next() => if needs_shutdown { break } else { continue },
            }
        }

        Ok(())
    }
}
