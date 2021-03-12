use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use async_raft::async_trait::async_trait;
use async_raft::raft::{
    AppendEntriesRequest, AppendEntriesResponse, ClientWriteRequest, InstallSnapshotRequest, InstallSnapshotResponse, VoteRequest, VoteResponse,
};
use async_raft::{error::ClientWriteError, RaftNetwork};
use tokio::sync::watch;
use tonic::transport::Channel;

use crate::ctl_raft::models::{
    AssignStreamReplicaToNode, CRCClientRequest, CRCRequest, RaftAssignStreamReplicaToNode, RaftClientRequest, RaftClientResponse,
    RaftUpdateSchemaRequest,
};
use crate::ctl_raft::CRC;
use crate::network::{ClientRequest, UpdateSchema};
use crate::network::{RaftAppendEntries, RaftInstallSnapshot, RaftVote};
use crate::proto::peer::{RaftAppendEntriesMsg, RaftInstallSnapshotMsg, RaftVoteMsg};
use crate::utils;
use crate::NodeId;
use crate::{ok_or_else_tx_err, raft_client_write};

const RAFT_CLUSTER: &str = "hadron";
const ERR_UNEXPECTED_RAFT_RESPONSE_VARIANT: &str = "error handling response from storage layer, unexpected variant";
const ERR_FORWARD_LEADER_UNKNOWN: &str = "error forwarding request, cluster leadership is transitioning";

impl CRC {
    #[tracing::instrument(level = "trace", skip(self, req))]
    pub(super) async fn handle_request_update_schema(&mut self, req: UpdateSchema) {
        let (raft, forward_tx) = (self.raft.clone(), self.forward_tx.clone());
        tokio::spawn(async move {
            let client_request = ClientWriteRequest::new(RaftClientRequest::UpdateSchema(RaftUpdateSchemaRequest {
                validated: req.validated,
                token_id: req.creds.id,
            }));
            raft_client_write!(raft: raft, forward_tx: forward_tx, req: req, client_request: client_request,
                success: RaftClientResponse::UpdateSchema(res),
                success_expr: { let _ = req.tx.send(Ok(res)); },
                forward: RaftClientRequest::UpdateSchema(inner),
                forward_expr: CRCClientRequest::UpdateSchema(UpdateSchema {
                    req: req.req,
                    validated: inner.validated,
                    tx: req.tx,
                    creds: req.creds,
                }),
            );
        });
    }

    #[tracing::instrument(level = "trace", skip(self, req))]
    pub(super) async fn handle_raft_append_entries_rpc(&mut self, req: RaftAppendEntries) {
        let msg: AppendEntriesRequest<RaftClientRequest> =
            ok_or_else_tx_err!(utils::decode_flexbuf(&req.req.get_ref().payload).context(utils::ERR_DECODE_RAFT_RPC), req);
        let raft = self.raft.clone();
        tokio::spawn(async move {
            let res = ok_or_else_tx_err!(raft.append_entries(msg).await, req);
            let res_payload = ok_or_else_tx_err!(utils::encode_flexbuf(&res).context(utils::ERR_ENCODE_RAFT_RPC), req);
            let _ = req.tx.send(Ok(RaftAppendEntriesMsg {
                cluster: RAFT_CLUSTER.into(),
                payload: res_payload,
            }));
        });
    }

    #[tracing::instrument(level = "trace", skip(self, req))]
    pub(super) async fn handle_raft_vote_rpc(&mut self, req: RaftVote) {
        let msg: VoteRequest = ok_or_else_tx_err!(utils::decode_flexbuf(&req.req.get_ref().payload).context(utils::ERR_DECODE_RAFT_RPC), req);
        let raft = self.raft.clone();
        tokio::spawn(async move {
            let res = ok_or_else_tx_err!(raft.vote(msg).await, req);
            let res_payload = ok_or_else_tx_err!(utils::encode_flexbuf(&res).context(utils::ERR_ENCODE_RAFT_RPC), req);
            let _ = req.tx.send(Ok(RaftVoteMsg {
                cluster: RAFT_CLUSTER.into(),
                payload: res_payload,
            }));
        });
    }

    #[tracing::instrument(level = "trace", skip(self, req))]
    pub(super) async fn handle_raft_install_snapshot_rpc(&mut self, req: RaftInstallSnapshot) {
        let msg: InstallSnapshotRequest =
            ok_or_else_tx_err!(utils::decode_flexbuf(&req.req.get_ref().payload).context(utils::ERR_DECODE_RAFT_RPC), req);
        let raft = self.raft.clone();
        tokio::spawn(async move {
            let res = ok_or_else_tx_err!(raft.install_snapshot(msg).await, req);
            let res_payload = ok_or_else_tx_err!(utils::encode_flexbuf(&res).context(utils::ERR_ENCODE_RAFT_RPC), req);
            let _ = req.tx.send(Ok(RaftInstallSnapshotMsg {
                cluster: RAFT_CLUSTER.into(),
                payload: res_payload,
            }));
        });
    }

    /// Handle a stream replica assignment request from the CPC.
    #[tracing::instrument(level = "trace", skip(self, req))]
    pub(super) async fn handle_cpc_stream_replica_assignment(&mut self, req: AssignStreamReplicaToNode) {
        let raft = self.raft.clone();
        tokio::spawn(async move {
            let client_request = ClientWriteRequest::new(RaftClientRequest::AssignStreamReplicaToNode(RaftAssignStreamReplicaToNode {
                change: req.change,
                replica: req.replica.id,
            }));
            match raft.client_write(client_request).await {
                Ok(res) => match res.data {
                    RaftClientResponse::AssignStreamReplicaToNode(res) => {
                        let _ = req.tx.send(Ok(res));
                    }
                    _ => {
                        let _ = req.tx.send(Err(anyhow!(ERR_UNEXPECTED_RAFT_RESPONSE_VARIANT)));
                    }
                },
                Err(err) => {
                    let _ = req.tx.send(Err(err.into()));
                }
            }
        });
    }

    /// Handle forwarding of requests to peer nodes.
    #[tracing::instrument(level = "trace", skip(self, req, node_opt))]
    pub(super) async fn handle_forward_request(&mut self, req: CRCClientRequest, node_opt: Option<NodeId>) {
        // Ensure we have the latest info on the current raft leader, else immediately respond with an error.
        let node = match node_opt.or_else(|| self.metrics.borrow().current_leader) {
            Some(node) => node,
            None => return CRCRequest::from(req).respond_with_error(anyhow!(ERR_FORWARD_LEADER_UNKNOWN)),
        };
        // Attempt to forward the request to the leader node.
        let chan = match self.get_peer_channel(&node).await {
            Ok(chan) => chan,
            Err(err) => return CRCRequest::from(req).respond_with_error(err),
        };
        tokio::spawn(async move { crate::network::forward_client_request(ClientRequest::from(req), chan).await });
    }
}

/// Hadron core Raft network.
pub struct HCoreNetwork {
    /// A mapping of peer nodes to their communication channels.
    peer_channels: watch::Receiver<Arc<HashMap<NodeId, Channel>>>,
}

impl HCoreNetwork {
    pub fn new(peer_channels: watch::Receiver<Arc<HashMap<NodeId, Channel>>>) -> Self {
        Self { peer_channels }
    }

    async fn get_peer_channel(&self, target: &u64) -> Result<Channel> {
        self.peer_channels
            .borrow()
            .get(&target)
            .cloned()
            .ok_or_else(|| anyhow!("no active connection to target peer"))
    }
}

#[async_trait]
impl RaftNetwork<RaftClientRequest> for HCoreNetwork {
    #[tracing::instrument(level = "trace", skip(self, rpc))]
    async fn append_entries(&self, target: u64, rpc: AppendEntriesRequest<RaftClientRequest>) -> Result<AppendEntriesResponse> {
        let chan = self.get_peer_channel(&target).await?;
        let payload = utils::encode_flexbuf(&rpc).context(utils::ERR_ENCODE_RAFT_RPC)?;
        crate::network::send_append_entries(RAFT_CLUSTER.into(), payload, chan).await
    }

    #[tracing::instrument(level = "trace", skip(self, rpc))]
    async fn install_snapshot(&self, target: u64, rpc: InstallSnapshotRequest) -> Result<InstallSnapshotResponse> {
        let chan = self.get_peer_channel(&target).await?;
        let payload = utils::encode_flexbuf(&rpc).context(utils::ERR_ENCODE_RAFT_RPC)?;
        crate::network::send_install_snapshot(RAFT_CLUSTER.into(), payload, chan).await
    }

    #[tracing::instrument(level = "trace", skip(self, rpc))]
    async fn vote(&self, target: u64, rpc: VoteRequest) -> Result<VoteResponse> {
        let chan = self.get_peer_channel(&target).await?;
        let payload = utils::encode_flexbuf(&rpc).context(utils::ERR_ENCODE_RAFT_RPC)?;
        crate::network::send_vote(RAFT_CLUSTER.into(), payload, chan).await
    }
}
