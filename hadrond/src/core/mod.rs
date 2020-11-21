mod network;
mod storage;

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use async_raft::{Raft, RaftMetrics};
use futures::FutureExt;
use tokio::stream::StreamExt;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use tonic::transport::Channel;

use crate::auth::{AuthError, Claims, UserRole};
use crate::config::Config;
use crate::core::network::{HCoreNetwork, RaftClientRequest, RaftClientResponse};
use crate::core::storage::{HCoreStorage, IndexUpdate};
use crate::network::{PipelineStageSub, StreamPub, StreamSub, StreamUnsub, Transaction, UpdateSchema};
use crate::network::{RaftAppendEntries, RaftInstallSnapshot, RaftVote};
use crate::utils;
use crate::NodeId;

/// The concrete Raft type used by Hadron core.
pub type HCoreRaft = Raft<RaftClientRequest, RaftClientResponse, HCoreNetwork, HCoreStorage>;

/// Hadron core data layer.
pub struct HCore {
    /// The ID of this node in the cluster.
    _id: NodeId,
    /// The application's runtime config.
    config: Arc<Config>,
    /// All active communication channels managed by the network layer.
    peer_channels: watch::Receiver<Arc<HashMap<NodeId, Channel>>>,
    /// A channel of requests coming in from clients & peers.
    requests: mpsc::UnboundedReceiver<HCoreRequest>,
    /// A channel used for receiving index updates form the storage layer.
    index_rx: mpsc::UnboundedReceiver<IndexUpdate>,
    /// A channel used for forwarding requests to a peer node.
    forward_tx: mpsc::UnboundedSender<(HCoreClientRequest, Option<NodeId>)>,
    /// A channel used for forwarding requests to a peer node.
    forward_rx: mpsc::UnboundedReceiver<(HCoreClientRequest, Option<NodeId>)>,
    /// The Raft instance used for mutating the storage layer.
    raft: HCoreRaft,
    /// A handle to the Raft's metrics channel.
    metrics: watch::Receiver<RaftMetrics>,

    _net: Arc<HCoreNetwork>,
    _storage: Arc<HCoreStorage>,

    /// An index of user permissions.
    index_user_permissions: HashMap<u64, UserRole>,
    /// An index of user permissions.
    index_token_permissions: HashMap<u64, Claims>,
}

impl HCore {
    pub async fn new(
        id: NodeId, config: Arc<Config>, peer_channels: watch::Receiver<Arc<HashMap<NodeId, Channel>>>,
        requests: mpsc::UnboundedReceiver<HCoreRequest>,
    ) -> Result<Self> {
        // Initialize network & storage interfaces.
        let net = Arc::new(HCoreNetwork::new(peer_channels.clone()));
        let (storage, index_rx) = HCoreStorage::new(id, config.clone()).await?;
        let storage = Arc::new(storage);

        // Recover some state from storage.
        let index_user_permissions = storage
            .recover_user_permissions()
            .await
            .context("error recovering user permissions state from storage")?;
        let index_token_permissions = storage
            .recover_token_permissions()
            .await
            .context("error recovering token permissions state from storage")?;

        // Initialize Raft.
        let raft_config = Arc::new(
            async_raft::Config::build("core".into())
                .heartbeat_interval(config.raft_heartbeat_interval_millis as u64)
                .election_timeout_min(config.raft_election_timeout_min as u64)
                .election_timeout_max(config.raft_election_timeout_max as u64)
                .validate()
                .context("invalid raft cluster configuration")?,
        );
        let raft = HCoreRaft::new(id, raft_config, net.clone(), storage.clone());
        let metrics = raft.metrics();

        let (forward_tx, forward_rx) = mpsc::unbounded_channel();
        Ok(Self {
            _id: id,
            config,
            peer_channels,
            requests,
            index_rx,
            forward_tx,
            forward_rx,
            raft,
            metrics,
            _net: net,
            _storage: storage,
            index_user_permissions,
            index_token_permissions,
        })
    }

    pub fn spawn(self) -> JoinHandle<()> {
        tokio::spawn(self.run())
    }

    async fn run(mut self) {
        let mut form_cluster_rx = Self::spawn_cluster_formation_delay_timer(self.config.clone()).fuse();
        loop {
            tokio::select! {
                Some(req) = self.requests.next() => self.handle_request(req).await,
                Some(update) = self.index_rx.next() => self.handle_index_update(update).await,
                Some(forward) = self.forward_rx.next() => self.handle_forward_request(forward.0, forward.1).await,
                Some(metrics) = self.metrics.next() => {
                    tracing::trace!(?metrics, "raft metrics update"); // TODO: wire up metrics.
                }
                Ok(_) = &mut form_cluster_rx => self.initialize_raft_cluster().await,
            }

            // After every iteration, we need to ensure that our indices are fully updated.
            while let Ok(update) = self.index_rx.try_recv() {
                self.handle_index_update(update).await;
            }
        }
    }

    #[tracing::instrument(level = "trace", skip(self, req))]
    async fn handle_request(&mut self, req: HCoreRequest) {
        match req {
            HCoreRequest::Client(HCoreClientRequest::Transaction(req)) => self.handle_request_transaction(req).await,
            HCoreRequest::Client(HCoreClientRequest::StreamPub(req)) => self.handle_request_stream_pub(req).await,
            HCoreRequest::Client(HCoreClientRequest::StreamSub(req)) => self.handle_request_stream_sub(req).await,
            HCoreRequest::Client(HCoreClientRequest::StreamUnsub(req)) => self.handle_request_stream_unsub(req).await,
            HCoreRequest::Client(HCoreClientRequest::PipelineStageSub(req)) => self.handle_request_pipeline_stage_sub(req).await,
            HCoreRequest::Client(HCoreClientRequest::UpdateSchema(req)) => self.handle_request_update_schema(req).await,
            HCoreRequest::RaftAppendEntries(req) => self.handle_raft_append_entries_rpc(req).await,
            HCoreRequest::RaftInstallSnapshot(req) => self.handle_raft_install_snapshot_rpc(req).await,
            HCoreRequest::RaftVote(req) => self.handle_raft_vote_rpc(req).await,
        }
    }

    /// Handle updates from the storage layer which should be used to update indices.
    #[tracing::instrument(level = "trace", skip(self, update))]
    async fn handle_index_update(&mut self, update: IndexUpdate) {
        // TODO: update indices
    }

    #[tracing::instrument(level = "trace", skip(self))]
    async fn initialize_raft_cluster(&mut self) {
        if self.metrics.borrow().current_term > 0 {
            tracing::trace!("skipping Raft cluster initialization as cluster is not pristine");
            return;
        }
        let nodes = self.peer_channels.borrow().keys().cloned().collect();
        tracing::trace!(?nodes, "initializing cluster");
        if let Err(err) = self.raft.initialize(nodes).await {
            tracing::error!(error = %err);
        }
    }

    /// Spawn a task which will delay for the configured cluster formation delay.
    ///
    /// This routine does not actually initialize the cluster, the main control loop should.
    #[tracing::instrument(level = "trace", skip(config))]
    fn spawn_cluster_formation_delay_timer(config: Arc<Config>) -> oneshot::Receiver<()> {
        // Setup a timeout after which we will initialize the Raft cluster with the current set
        // of connected peers.
        let (tx, rx) = oneshot::channel();
        let delay = config.initial_cluster_formation_delay();
        tracing::trace!(delay = ?delay, "delaying cluster formation");
        tokio::spawn(async move {
            tokio::time::delay_for(delay).await;
            let _ = tx.send(());
        });
        rx
    }

    /// Get the given token's claims, else return an auth error.
    pub(self) fn must_get_token_claims(&self, token_id: &u64) -> Result<&Claims> {
        match self.index_token_permissions.get(token_id) {
            Some(claims) => Ok(claims),
            None => Err(AuthError::UnknownToken.into()),
        }
    }

    pub(self) async fn get_peer_channel(&self, target: &u64) -> Result<Channel> {
        self.peer_channels
            .borrow()
            .get(&target)
            .cloned()
            .ok_or_else(|| anyhow!("no active connection to target peer"))
    }
}

/// All request variants which the Hadron core data layer can handle.
pub enum HCoreRequest {
    Client(HCoreClientRequest),
    RaftAppendEntries(RaftAppendEntries),
    RaftVote(RaftVote),
    RaftInstallSnapshot(RaftInstallSnapshot),
}

impl HCoreRequest {
    pub(super) fn respond_with_error(self, err: anyhow::Error) {
        match self {
            HCoreRequest::Client(HCoreClientRequest::Transaction(val)) => {
                let _ = val.tx.send(utils::map_result_to_status(Err(err)));
            }
            HCoreRequest::Client(HCoreClientRequest::StreamPub(val)) => {
                let _ = val.tx.send(utils::map_result_to_status(Err(err)));
            }
            HCoreRequest::Client(HCoreClientRequest::StreamSub(val)) => {
                let _ = val.tx.send(utils::map_result_to_status(Err(err)));
            }
            HCoreRequest::Client(HCoreClientRequest::StreamUnsub(val)) => {
                let _ = val.tx.send(utils::map_result_to_status(Err(err)));
            }
            HCoreRequest::Client(HCoreClientRequest::PipelineStageSub(val)) => {
                let _ = val.tx.send(utils::map_result_to_status(Err(err)));
            }
            HCoreRequest::Client(HCoreClientRequest::UpdateSchema(val)) => {
                let _ = val.tx.send(utils::map_result_to_status(Err(err)));
            }
            HCoreRequest::RaftAppendEntries(val) => {
                let _ = val.tx.send(utils::map_result_to_status(Err(err)));
            }
            HCoreRequest::RaftVote(val) => {
                let _ = val.tx.send(utils::map_result_to_status(Err(err)));
            }
            HCoreRequest::RaftInstallSnapshot(val) => {
                let _ = val.tx.send(utils::map_result_to_status(Err(err)));
            }
        }
    }
}

impl From<HCoreClientRequest> for HCoreRequest {
    fn from(src: HCoreClientRequest) -> Self {
        HCoreRequest::Client(src)
    }
}

/// All requests variants which the Hadron core data layer can handle, specifically those which
/// can come from clients.
pub enum HCoreClientRequest {
    Transaction(Transaction),
    StreamPub(StreamPub),
    StreamSub(StreamSub),
    StreamUnsub(StreamUnsub),
    PipelineStageSub(PipelineStageSub),
    UpdateSchema(UpdateSchema),
}
