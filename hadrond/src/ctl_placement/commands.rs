//! CPC async tasks.

use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use crate::ctl_placement::Task;
use crate::ctl_raft::models::{AssignStreamReplicaToNode, CRCRequest};
use crate::models::placement::{Assignment, PipelineReplica, StreamReplica};
use crate::NodeId;

/// CPC command variants.
#[derive(Clone)]
pub(super) enum CommandType {
    /// Assign the given stream replica to the target node.
    AssignStreamReplicaToNode { assignment: Assignment, replica: Arc<StreamReplica> },
}

/// A CPC command to some other components within the cluster.
pub(super) struct Command {
    /// A channel for sending requests to the CRC.
    crc_tx: mpsc::UnboundedSender<CRCRequest>,
    /// A channel of CPC tasks.
    cpc_tasks: mpsc::UnboundedSender<Task>,
    /// The type of command to execute.
    r#type: CommandType,
}

impl Command {
    /// Create a new command to be spawned for execution.
    pub fn new(crc_tx: mpsc::UnboundedSender<CRCRequest>, cpc_tasks: mpsc::UnboundedSender<Task>, r#type: CommandType) -> Self {
        Self { crc_tx, cpc_tasks, r#type }
    }

    /// Update the cluster Raft with an assignment request, placing the given
    /// stream replica onto the target node.
    ///
    /// The flow of data here is unidirectional. So when the update succeeds, the CRC will emit
    /// corresponding events which will update the state of the CPC.
    pub fn assign_stream_replica_to_node(
        crc_tx: mpsc::UnboundedSender<CRCRequest>, cpc_tasks: mpsc::UnboundedSender<Task>, assignment: Assignment, replica: Arc<StreamReplica>,
    ) -> Self {
        let r#type = CommandType::AssignStreamReplicaToNode { assignment, replica };
        Self { crc_tx, cpc_tasks, r#type }
    }

    /// Spawn and execute this command.
    pub fn spawn(self) -> JoinHandle<()> {
        tokio::spawn(async move {
            let (cmd, cpc) = (self.r#type.clone(), self.cpc_tasks.clone());
            let res = match self.r#type {
                CommandType::AssignStreamReplicaToNode { assignment, replica } => {
                    assign_stream_replica_to_node(assignment, replica, self.crc_tx.clone(), self.cpc_tasks.clone()).await
                }
            };
            match res {
                Ok(_) => (),
                Err(err) => {
                    // Send the failed command back to the CPC to be processed.
                    tracing::error!(error = ?err, "error executing CPC command");
                    tokio::time::delay_for(std::time::Duration::from_secs(2)).await; // Don't spam.
                    let _ = cpc.send(Task::CommandError(cmd));
                }
            }
        })
    }
}

#[tracing::instrument(level = "trace", skip(crc_tx, cpc_tasks), err)]
async fn assign_stream_replica_to_node(
    assignment: Assignment, replica: Arc<StreamReplica>, crc_tx: mpsc::UnboundedSender<CRCRequest>, cpc_tasks: mpsc::UnboundedSender<Task>,
) -> Result<()> {
    tracing::debug!("task started");
    let (tx, rx) = oneshot::channel();
    let req = CRCRequest::Placement(AssignStreamReplicaToNode {
        change: assignment,
        replica,
        tx,
    });
    crc_tx.send(req).map_err(|_| anyhow!("unable to communicate with CRC"))?;
    let res = rx
        .await
        .context("no response received from CRC")?
        .context("error while submitting stream replica assignment to CRC")?;
    tracing::debug!("task complete");
    Ok(())
}
