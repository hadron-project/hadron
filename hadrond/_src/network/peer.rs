//! The Peer gRPC service implementation module.

use anyhow::{Context, Result};
use async_raft::raft::{AppendEntriesResponse, InstallSnapshotResponse, VoteResponse};
use tokio::sync::mpsc;
use tokio::sync::oneshot::{channel, Sender};
use tonic::{async_trait, transport::Channel, Request, Response};

pub use crate::proto::peer::peer_client::PeerClient;
use crate::proto::peer::peer_server::Peer;
pub use crate::proto::peer::peer_server::PeerServer;
use crate::proto::peer::{HandshakeMsg, RaftAppendEntriesMsg, RaftInstallSnapshotMsg, RaftVoteMsg};
use crate::utils::{self, status_from_rcv_error, TonicResult};
use crate::NodeId;

const ERR_PEER_RPC_FAILURE: &str = "error response returned from peer RPC";

pub(super) struct PeerService {
    /// This node's ID.
    id: NodeId,
    /// The channel used for propagating requests into the network actor, which flows into the rest of the system.
    network: mpsc::UnboundedSender<PeerRequest>, // TODO: In the future, we will want to document this & make capacity configurable.
}

impl PeerService {
    pub fn new(id: NodeId, network: mpsc::UnboundedSender<PeerRequest>) -> Self {
        Self { id, network }
    }
}

#[async_trait]
impl Peer for PeerService {
    #[tracing::instrument(level = "trace", skip(self, _request))]
    async fn handshake(&self, _request: Request<HandshakeMsg>) -> TonicResult<Response<HandshakeMsg>> {
        tracing::trace!("handling handshake request");
        Ok(Response::new(HandshakeMsg { node_id: self.id }))
    }

    async fn raft_append_entries(&self, req: Request<RaftAppendEntriesMsg>) -> TonicResult<Response<RaftAppendEntriesMsg>> {
        let (tx, rx) = channel();
        let _ = self.network.send(PeerRequest::RaftAppendEntries(RaftAppendEntries { req, tx }));
        rx.await.map_err(status_from_rcv_error).and_then(|res| res).map(Response::new)
    }

    async fn raft_vote(&self, req: Request<RaftVoteMsg>) -> TonicResult<Response<RaftVoteMsg>> {
        let (tx, rx) = channel();
        let _ = self.network.send(PeerRequest::RaftVote(RaftVote { req, tx }));
        rx.await.map_err(status_from_rcv_error).and_then(|res| res).map(Response::new)
    }

    async fn raft_install_snapshot(&self, req: Request<RaftInstallSnapshotMsg>) -> TonicResult<Response<RaftInstallSnapshotMsg>> {
        let (tx, rx) = channel();
        let _ = self.network.send(PeerRequest::RaftInstallSnapshot(RaftInstallSnapshot { req, tx }));
        rx.await.map_err(status_from_rcv_error).and_then(|res| res).map(Response::new)
    }
}

/// A request flowing into this node from a peer server.
pub enum PeerRequest {
    RaftAppendEntries(RaftAppendEntries),
    RaftVote(RaftVote),
    RaftInstallSnapshot(RaftInstallSnapshot),
}

pub struct RaftAppendEntries {
    pub req: Request<RaftAppendEntriesMsg>,
    pub tx: Sender<TonicResult<RaftAppendEntriesMsg>>,
}

pub struct RaftVote {
    pub req: Request<RaftVoteMsg>,
    pub tx: Sender<TonicResult<RaftVoteMsg>>,
}

pub struct RaftInstallSnapshot {
    pub req: Request<RaftInstallSnapshotMsg>,
    pub tx: Sender<TonicResult<RaftInstallSnapshotMsg>>,
}

/// Send a Raft AppendEntries RPC to the peer on the other side of the given channel.
#[tracing::instrument(level = "trace", skip(cluster, payload, chan))]
pub async fn send_append_entries(cluster: String, payload: Vec<u8>, chan: Channel) -> Result<AppendEntriesResponse> {
    let mut client = PeerClient::new(chan);
    let response_payload = client
        .raft_append_entries(RaftAppendEntriesMsg { cluster, payload })
        .await
        .context(ERR_PEER_RPC_FAILURE)?
        .into_inner()
        .payload;
    utils::decode_flexbuf(&response_payload).context(utils::ERR_DECODE_RAFT_RPC_RESPONSE)
}

/// Send a Raft InstallSnapshot RPC to the peer on the other side of the given channel.
#[tracing::instrument(level = "trace", skip(cluster, payload, chan))]
pub async fn send_install_snapshot(cluster: String, payload: Vec<u8>, chan: Channel) -> Result<InstallSnapshotResponse> {
    let mut client = PeerClient::new(chan);
    let response_payload = client
        .raft_install_snapshot(RaftInstallSnapshotMsg { cluster, payload })
        .await
        .context(ERR_PEER_RPC_FAILURE)?
        .into_inner()
        .payload;
    utils::decode_flexbuf(&response_payload).context(utils::ERR_DECODE_RAFT_RPC_RESPONSE)
}

/// Send a Raft Vote RPC to the peer on the other side of the given channel.
#[tracing::instrument(level = "trace", skip(cluster, payload, chan))]
pub async fn send_vote(cluster: String, payload: Vec<u8>, chan: Channel) -> Result<VoteResponse> {
    let mut client = PeerClient::new(chan);
    let response_payload = client
        .raft_vote(RaftVoteMsg { cluster, payload })
        .await
        .context(ERR_PEER_RPC_FAILURE)?
        .into_inner()
        .payload;
    utils::decode_flexbuf(&response_payload).context(utils::ERR_DECODE_RAFT_RPC_RESPONSE)
}
