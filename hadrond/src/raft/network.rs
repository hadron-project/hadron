use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use async_raft::async_trait::async_trait;
use async_raft::raft::{AppendEntriesRequest, AppendEntriesResponse, InstallSnapshotRequest, InstallSnapshotResponse, VoteRequest, VoteResponse};
use async_raft::AppData;
use async_raft::RaftNetwork;
use tokio::sync::watch;
use tonic::transport::Channel;

use crate::utils;
use crate::NodeId;

/// Hadron core Raft network.
pub struct TonicNetworkProvider<Req: AppData> {
    /// A mapping of peer nodes to their communication channels.
    peer_channels: watch::Receiver<Arc<HashMap<NodeId, Channel>>>,
    /// The name of the associated Raft cluster.
    cluster_name: String,
    mark_req: std::marker::PhantomData<Req>,
}

impl<Req: AppData> TonicNetworkProvider<Req> {
    /// Create a new instance.
    pub fn new(cluster_name: String, peer_channels: watch::Receiver<Arc<HashMap<NodeId, Channel>>>) -> Self {
        let mark_req = std::marker::PhantomData;
        Self {
            peer_channels,
            cluster_name,
            mark_req,
        }
    }

    fn get_peer_channel(&self, target: &u64) -> Result<Channel> {
        self.peer_channels
            .borrow()
            .get(&target)
            .cloned()
            .ok_or_else(|| anyhow!("no active connection to target peer"))
    }
}

#[async_trait]
impl<Req: AppData> RaftNetwork<Req> for TonicNetworkProvider<Req> {
    #[tracing::instrument(level = "trace", skip(self, rpc))]
    async fn append_entries(&self, target: u64, rpc: AppendEntriesRequest<Req>) -> Result<AppendEntriesResponse> {
        let chan = self.get_peer_channel(&target)?;
        let payload = utils::encode_flexbuf(&rpc).context(utils::ERR_ENCODE_RAFT_RPC)?;
        crate::network::send_append_entries(self.cluster_name.clone(), payload, chan).await
    }

    #[tracing::instrument(level = "trace", skip(self, rpc))]
    async fn install_snapshot(&self, target: u64, rpc: InstallSnapshotRequest) -> Result<InstallSnapshotResponse> {
        let chan = self.get_peer_channel(&target)?;
        let payload = utils::encode_flexbuf(&rpc).context(utils::ERR_ENCODE_RAFT_RPC)?;
        crate::network::send_install_snapshot(self.cluster_name.clone(), payload, chan).await
    }

    #[tracing::instrument(level = "trace", skip(self, rpc))]
    async fn vote(&self, target: u64, rpc: VoteRequest) -> Result<VoteResponse> {
        let chan = self.get_peer_channel(&target)?;
        let payload = utils::encode_flexbuf(&rpc).context(utils::ERR_ENCODE_RAFT_RPC)?;
        crate::network::send_vote(self.cluster_name.clone(), payload, chan).await
    }
}
