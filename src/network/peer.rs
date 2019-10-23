//! The Peer gRPC service implementation module.

use anyhow::Result;
use tokio::sync::mpsc;
use tokio::sync::oneshot::{channel, Sender};

use crate::network::{map_result_to_status, status_from_rcv_error, TonicResult};
pub use crate::proto::peer::peer_client::PeerClient;
use crate::proto::peer::peer_server::Peer;
pub use crate::proto::peer::peer_server::PeerServer;
use crate::proto::peer::{HandshakeMsg, RaftAppendEntriesMsg, RaftInstallSnapshotMsg, RaftVoteMsg};
use crate::NodeId;

use tonic::{async_trait, Request, Response};

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
    #[tracing::instrument(level = "trace", skip(self))]
    async fn handshake(&self, request: Request<HandshakeMsg>) -> TonicResult<Response<HandshakeMsg>> {
        tracing::trace!("handling handshake request");
        Ok(Response::new(HandshakeMsg { node_id: self.id }))
    }

    async fn raft_append_entries(&self, request: Request<RaftAppendEntriesMsg>) -> TonicResult<Response<RaftAppendEntriesMsg>> {
        let (tx, rx) = channel();
        let msg = PeerRequest::RaftAppendEntries(request, tx);
        let _ = self.network.send(msg);
        rx.await.map_err(status_from_rcv_error).and_then(map_result_to_status).map(Response::new)
    }

    async fn raft_vote(&self, request: Request<RaftVoteMsg>) -> TonicResult<Response<RaftVoteMsg>> {
        let (tx, rx) = channel();
        let msg = PeerRequest::RaftVote(request, tx);
        let _ = self.network.send(msg);
        rx.await.map_err(status_from_rcv_error).and_then(map_result_to_status).map(Response::new)
    }

    async fn raft_install_snapshot(&self, request: Request<RaftInstallSnapshotMsg>) -> TonicResult<Response<RaftInstallSnapshotMsg>> {
        let (tx, rx) = channel();
        let msg = PeerRequest::RaftInstallSnapshot(request, tx);
        let _ = self.network.send(msg);
        rx.await.map_err(status_from_rcv_error).and_then(map_result_to_status).map(Response::new)
    }
}

/// A request flowing into this node from a peer server.
pub(super) enum PeerRequest {
    RaftAppendEntries(Request<RaftAppendEntriesMsg>, Sender<Result<RaftAppendEntriesMsg>>),
    RaftVote(Request<RaftVoteMsg>, Sender<Result<RaftVoteMsg>>),
    RaftInstallSnapshot(Request<RaftInstallSnapshotMsg>, Sender<Result<RaftInstallSnapshotMsg>>),
}
