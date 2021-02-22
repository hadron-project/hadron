//! CPC async tasks.

use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;

use crate::ctl_placement::Task;
use crate::ctl_raft::models::{AssignStreamReplicaToNode, CRCRequest};
use crate::models::placement::{Assignment, PipelineReplica, StreamReplica};
use crate::NodeId;

/// Update the cluster Raft with an assignment request, placing the given
/// stream replica onto the target node.
///
/// The flow of data here is unidirectional. So when the update succeeds, the CRC will emit
/// corresponding events which will update the state of the CPC.
#[tracing::instrument(level = "trace", skip(crc_tx, cpc_tasks), err)]
pub(super) async fn assign_stream_replica_to_node(
    change: Assignment, replica: Arc<StreamReplica>, crc_tx: mpsc::UnboundedSender<CRCRequest>, cpc_tasks: mpsc::UnboundedSender<Task>,
) -> Result<()> {
    tracing::trace!("task started");
    let (tx, rx) = oneshot::channel();
    let req = CRCRequest::Placement(AssignStreamReplicaToNode { change, replica, tx });
    crc_tx.send(req).map_err(|_| anyhow!("unable to communicate with CRC"))?;
    let res = rx.await.context("no response received from CRC")?;
    match res {
        Ok(_) => tracing::trace!("task complete"),
        Err(err) => {
            tracing::error!(error = ?err, "error while submitting stream replica assignment to CRC");
        }
    }
    Ok(())
}
