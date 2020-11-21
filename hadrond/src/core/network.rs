use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use async_raft::async_trait::async_trait;
use async_raft::raft::{
    AppendEntriesRequest, AppendEntriesResponse, ClientWriteRequest, InstallSnapshotRequest, InstallSnapshotResponse, VoteRequest, VoteResponse,
};
use async_raft::{error::ClientWriteError, AppData, AppDataResponse, RaftNetwork};
use serde::{Deserialize, Serialize};
use tokio::sync::{oneshot, watch};
use tonic::transport::Channel;

use crate::core::{HCore, HCoreClientRequest, HCoreRequest};
use crate::network::{ClientRequest, PipelineStageSub, StreamPub, StreamSub, StreamUnsub, Transaction, UpdateSchema};
use crate::network::{RaftAppendEntries, RaftInstallSnapshot, RaftVote};
use crate::proto::client::{PipelineStageSubClient, PipelineStageSubServer, StreamUnsubRequest, StreamUnsubResponse};
use crate::proto::client::{StreamPubRequest, StreamPubResponse, StreamSubClient, StreamSubServer};
use crate::proto::client::{TransactionClient, TransactionServer, UpdateSchemaRequest, UpdateSchemaResponse};
use crate::proto::peer::{RaftAppendEntriesMsg, RaftInstallSnapshotMsg, RaftVoteMsg};
use crate::utils;
use crate::NodeId;

const RAFT_CLUSTER: &str = "hadron";
const ERR_UNEXPECTED_RAFT_RESPONSE_VARIANT: &str = "error handling response from storage layer, unexpected variant";
const ERR_FORWARD_LEADER_UNKNOWN: &str = "error forwarding request, cluster leader unknown";

macro_rules! ok_or_else_tx_err {
    ($matcher:expr, $holder:ident) => {
        match $matcher {
            Ok(val) => val,
            Err(err) => {
                tracing::error!(error = %err);
                let _ = $holder.tx.send(utils::map_result_to_status(Err(err.into())));
                return;
            }
        }
    };
}

/// The Raft request type used for the Hadron core Raft.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum RaftClientRequest {
    Transaction(TransactionClient),
    StreamPub(StreamPubRequest),
    StreamSub(StreamSubClient),
    StreamUnsub(StreamUnsubRequest),
    PipelineStageSub(PipelineStageSubClient),
    UpdateSchema(UpdateSchemaRequest),
}

impl AppData for RaftClientRequest {}

/// The Raft response type used for the Hadron core Raft.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum RaftClientResponse {
    Transaction(TransactionServer),
    StreamPub(StreamPubResponse),
    StreamSub(StreamSubServer),
    StreamUnsub(StreamUnsubResponse),
    PipelineStageSub(PipelineStageSubServer),
    UpdateSchema(UpdateSchemaResponse),
}

impl AppDataResponse for RaftClientResponse {}

impl HCore {
    #[tracing::instrument(level = "trace", skip(self, req))]
    pub(super) async fn handle_request_transaction(&mut self, req: Transaction) {
        todo!("")
    }

    #[tracing::instrument(level = "trace", skip(self, req))]
    pub(super) async fn handle_request_stream_pub(&mut self, req: StreamPub) {
        let claims = ok_or_else_tx_err!(self.must_get_token_claims(&req.creds.id), req);
        ok_or_else_tx_err!(claims.check_stream_pub_auth(&req.req.stream), req);
        let (raft, forward_tx) = (self.raft.clone(), self.forward_tx.clone());
        tokio::spawn(async move {
            let client_request = ClientWriteRequest::new(RaftClientRequest::StreamPub(req.req));
            match raft.client_write(client_request).await {
                Ok(res) => match res.data {
                    RaftClientResponse::StreamPub(res) => {
                        let _ = req.tx.send(Ok(res));
                    }
                    _ => {
                        let _ = req
                            .tx
                            .send(utils::map_result_to_status(Err(anyhow!(ERR_UNEXPECTED_RAFT_RESPONSE_VARIANT))));
                    }
                },
                Err(err) => match err {
                    ClientWriteError::RaftError(err) => {
                        let _ = req.tx.send(utils::map_result_to_status(Err(err.into())));
                    }
                    ClientWriteError::ForwardToLeader(orig_req, node_opt) => match orig_req {
                        RaftClientRequest::StreamPub(req_fwd) => {
                            let _ = forward_tx.send((
                                HCoreClientRequest::StreamPub(StreamPub {
                                    req: req_fwd,
                                    tx: req.tx,
                                    creds: req.creds,
                                }),
                                node_opt,
                            ));
                        }
                        _ => {
                            let _ = req
                                .tx
                                .send(utils::map_result_to_status(Err(anyhow!(ERR_UNEXPECTED_RAFT_RESPONSE_VARIANT))));
                        }
                    },
                },
            }
        });
    }

    #[tracing::instrument(level = "trace", skip(self, req))]
    pub(super) async fn handle_request_stream_sub(&mut self, req: StreamSub) {
        todo!("")
    }

    #[tracing::instrument(level = "trace", skip(self, req))]
    pub(super) async fn handle_request_stream_unsub(&mut self, req: StreamUnsub) {
        todo!("")
    }

    #[tracing::instrument(level = "trace", skip(self, req))]
    pub(super) async fn handle_request_pipeline_stage_sub(&mut self, req: PipelineStageSub) {
        todo!("")
    }

    #[tracing::instrument(level = "trace", skip(self, req))]
    pub(super) async fn handle_request_update_schema(&mut self, req: UpdateSchema) {
        todo!("")
    }

    #[tracing::instrument(level = "trace", skip(self, req))]
    pub(super) async fn handle_raft_append_entries_rpc(&mut self, req: RaftAppendEntries) {
        let msg: AppendEntriesRequest<RaftClientRequest> =
            ok_or_else_tx_err!(utils::bin_decode(&req.req.get_ref().payload).context(utils::ERR_DECODE_RAFT_RPC), req);
        let raft = self.raft.clone();
        tokio::spawn(async move {
            let res = ok_or_else_tx_err!(raft.append_entries(msg).await, req);
            let res_payload = ok_or_else_tx_err!(utils::bin_encode(&res).context(utils::ERR_ENCODE_RAFT_RPC), req);
            let _ = req.tx.send(Ok(RaftAppendEntriesMsg {
                cluster: RAFT_CLUSTER.into(),
                payload: res_payload,
            }));
        });
    }

    #[tracing::instrument(level = "trace", skip(self, req))]
    pub(super) async fn handle_raft_vote_rpc(&mut self, req: RaftVote) {
        let msg: VoteRequest = ok_or_else_tx_err!(utils::bin_decode(&req.req.get_ref().payload).context(utils::ERR_DECODE_RAFT_RPC), req);
        let raft = self.raft.clone();
        tokio::spawn(async move {
            let res = ok_or_else_tx_err!(raft.vote(msg).await, req);
            let res_payload = ok_or_else_tx_err!(utils::bin_encode(&res).context(utils::ERR_ENCODE_RAFT_RPC), req);
            let _ = req.tx.send(Ok(RaftVoteMsg {
                cluster: RAFT_CLUSTER.into(),
                payload: res_payload,
            }));
        });
    }

    #[tracing::instrument(level = "trace", skip(self, req))]
    pub(super) async fn handle_raft_install_snapshot_rpc(&mut self, req: RaftInstallSnapshot) {
        let msg: InstallSnapshotRequest = ok_or_else_tx_err!(utils::bin_decode(&req.req.get_ref().payload).context(utils::ERR_DECODE_RAFT_RPC), req);
        let raft = self.raft.clone();
        tokio::spawn(async move {
            let res = ok_or_else_tx_err!(raft.install_snapshot(msg).await, req);
            let res_payload = ok_or_else_tx_err!(utils::bin_encode(&res).context(utils::ERR_ENCODE_RAFT_RPC), req);
            let _ = req.tx.send(Ok(RaftInstallSnapshotMsg {
                cluster: RAFT_CLUSTER.into(),
                payload: res_payload,
            }));
        });
    }

    /// Handle forwarding of requests to peer nodes.
    #[tracing::instrument(level = "trace", skip(self, req, node_opt))]
    pub(super) async fn handle_forward_request(&mut self, req: HCoreClientRequest, node_opt: Option<NodeId>) {
        // Ensure we have the latest info on the current raft leader, else immediately respond with an error.
        let node = match node_opt.or_else(|| self.metrics.borrow().current_leader) {
            Some(node) => node,
            None => return HCoreRequest::from(req).respond_with_error(anyhow!(ERR_FORWARD_LEADER_UNKNOWN)),
        };
        // Attempt to forward the request to the leader node.
        let chan = match self.get_peer_channel(&node).await {
            Ok(chan) => chan,
            Err(err) => return HCoreRequest::from(req).respond_with_error(err),
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
        let payload = utils::bin_encode(&rpc).context(utils::ERR_ENCODE_RAFT_RPC)?;
        crate::network::send_append_entries(RAFT_CLUSTER.into(), payload, chan).await
    }

    #[tracing::instrument(level = "trace", skip(self, rpc))]
    async fn install_snapshot(&self, target: u64, rpc: InstallSnapshotRequest) -> Result<InstallSnapshotResponse> {
        let chan = self.get_peer_channel(&target).await?;
        let payload = utils::bin_encode(&rpc).context(utils::ERR_ENCODE_RAFT_RPC)?;
        crate::network::send_install_snapshot(RAFT_CLUSTER.into(), payload, chan).await
    }

    #[tracing::instrument(level = "trace", skip(self, rpc))]
    async fn vote(&self, target: u64, rpc: VoteRequest) -> Result<VoteResponse> {
        let chan = self.get_peer_channel(&target).await?;
        let payload = utils::bin_encode(&rpc).context(utils::ERR_ENCODE_RAFT_RPC)?;
        crate::network::send_vote(RAFT_CLUSTER.into(), payload, chan).await
    }
}

impl From<HCoreClientRequest> for ClientRequest {
    fn from(src: HCoreClientRequest) -> Self {
        match src {
            HCoreClientRequest::Transaction(req) => ClientRequest::Transaction(req),
            HCoreClientRequest::StreamPub(req) => ClientRequest::StreamPub(req),
            HCoreClientRequest::StreamSub(req) => ClientRequest::StreamSub(req),
            HCoreClientRequest::StreamUnsub(req) => ClientRequest::StreamUnsub(req),
            HCoreClientRequest::PipelineStageSub(req) => ClientRequest::PipelineStageSub(req),
            HCoreClientRequest::UpdateSchema(req) => ClientRequest::UpdateSchema(req),
        }
    }
}
