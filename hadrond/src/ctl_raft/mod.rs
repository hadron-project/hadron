//! Cluster Raft Controller (CRC)
//!
//! The CRC is responsible for controlling the state of the Hadron cluster as a whole, and
//! exposes a control signal to the application in order to drive the system. All cluster-wide
//! changes go through Raft. These types of changes include but are not limited to:
//!
//! - Hadron cluster membership.
//! - Leadership designations for other control groups within the Hadron cluster.
//! - Schema management for user defined resources (Namespaces, Streams &c).
//! - AuthN & authZ resources such as users and tokens.
//!
//! ## Control Groups
//! The CRC manages other control groups within the Hadron system. Controllers form groups based
//! on the controller type, and other assignments from the CRC. All control group assignments go
//! through the CRC Raft, and all nodes in the cluster use that data from Raft to drive control
//! group resources.
//!
//! ### CRC Control Group Heartbeats
//! The CRC uses a cluster-wide Raft. The AppendEntries RPC system is used to detect when nodes in the
//! cluster begin to fail heartbeats. This data is immediately available to the CRC leader, which uses
//! this data to drive the control group leader nomination algorithm. The heartbeat algorithm follows:
//!
//! - Raft uses the AppendEntries RPC system to heartbeat members of the cluster.
//! - The CRC uses this same heartbeat system to detect when follower nodes of the cluster are failing
//!   to respond to heartbeats.
//! - If too many consecutive heartbeat failures occur, the CRC will reckon that node as being down
//!   with respect to control group leadership. If that node is a control group leader, then the CRC
//!   will need to nominate a new leader.
//!
//! ### CRC Control Group Leader Nomination Algorithm
//! Leadership nomination from the CRC for other control groups is based on a monotonically
//! increasing term per control group, which is disjoint from Raft's leadership terms. Conflicts
//! between leadership designation is easily and safely resolved based on designated leadership
//! terms. The algorithm is as follows:
//!
//! - CRC detects a dead control group leader & will immediately commit a Raft entry which
//!   increments the term for the control group. Other nodes will detect this change via the
//!   cluster Raft & then the members of the control group will no longer accept replication
//!   requests from the old leader if it were to restart.
//! - Once that Raft term change has committed, the control group will be without a leader. The
//!   CRC will then query all control group members asking for commit index & current term. Once
//!   a majority of nodes have responded with the correct term, the CRC will select a new leader
//!   from the group bearing the majority consensus on the commit index. That nomination will go
//!   through Raft.
//! - All control group members will receive the new leader nomination for the term, and will then
//!   continue as normal.

#![allow(dead_code)] // TODO: remove this

pub mod events;
mod macros;
pub mod models;
mod network;
mod storage;

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use async_raft::{Raft, RaftMetrics, SnapshotPolicy};
use futures::FutureExt;
use tokio::stream::StreamExt;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;
use tonic::transport::Channel;

use crate::config::Config;
use crate::ctl_raft::events::CRCEvent;
use crate::ctl_raft::models::{CRCClientRequest, CRCRequest, RaftClientRequest, RaftClientResponse};
use crate::ctl_raft::network::HCoreNetwork;
use crate::ctl_raft::storage::HCoreStorage;
use crate::database::Database;
use crate::NodeId;

pub use storage::CRCIndex;

/// The concrete Raft type used by Hadron core.
pub type CRCRaft = Raft<RaftClientRequest, RaftClientResponse, HCoreNetwork, HCoreStorage>;

/// Cluster Raft Controller.
///
/// This controller is responsible for all cluster-wide changes, all of which go through Raft.
pub struct CRC {
    /// The ID of this node in the cluster.
    id: NodeId,
    /// The application's runtime config.
    config: Arc<Config>,
    /// All active communication channels managed by the network layer.
    peer_channels: watch::Receiver<Arc<HashMap<NodeId, Channel>>>,
    /// A channel of requests coming in from clients & peers.
    requests: mpsc::UnboundedReceiver<CRCRequest>,
    /// A channel used for forwarding requests to a peer node.
    forward_tx: mpsc::UnboundedSender<(CRCClientRequest, Option<NodeId>)>,
    /// A channel used for forwarding requests to a peer node.
    forward_rx: mpsc::UnboundedReceiver<(CRCClientRequest, Option<NodeId>)>,
    /// The Raft instance used for mutating the storage layer.
    raft: CRCRaft,
    /// A handle to the Raft's metrics channel.
    metrics: watch::Receiver<RaftMetrics>,
    /// Application shutdown channel.
    shutdown: watch::Receiver<bool>,

    net: Arc<HCoreNetwork>,
    pub(self) storage: Arc<HCoreStorage>,
}

impl CRC {
    pub async fn new(
        id: NodeId, config: Arc<Config>, db: Database, peer_channels: watch::Receiver<Arc<HashMap<NodeId, Channel>>>,
        requests: mpsc::UnboundedReceiver<CRCRequest>, shutdown: watch::Receiver<bool>,
    ) -> Result<(Self, Arc<CRCIndex>, mpsc::UnboundedReceiver<CRCEvent>, watch::Receiver<RaftMetrics>)> {
        // Initialize network & storage interfaces.
        let net = Arc::new(HCoreNetwork::new(peer_channels.clone()));
        let (storage, index, events_rx) = HCoreStorage::new(id, config.clone(), db).await?;
        let storage = Arc::new(storage);

        // Initialize Raft.
        let raft_config = Arc::new(
            async_raft::Config::build("core".into())
                .heartbeat_interval(config.raft_heartbeat_interval_millis as u64)
                .election_timeout_min(config.raft_election_timeout_min as u64)
                .election_timeout_max(config.raft_election_timeout_max as u64)
                .snapshot_policy(SnapshotPolicy::LogsSinceLast(200_000)) // TODO: make this configurable.
                .validate()
                .context("invalid raft cluster configuration")?,
        );
        let raft = CRCRaft::new(id, raft_config, net.clone(), storage.clone());
        let metrics = raft.metrics();

        let (forward_tx, forward_rx) = mpsc::unbounded_channel();
        let this = Self {
            id,
            config,
            peer_channels,
            requests,
            forward_tx,
            forward_rx,
            raft,
            metrics: metrics.clone(),
            shutdown,
            net,
            storage,
        };
        Ok((this, index, events_rx, metrics))
    }

    pub fn spawn(self) -> JoinHandle<()> {
        tokio::spawn(self.run())
    }

    async fn run(mut self) {
        tracing::debug!("Cluster Raft Controller is online");

        let mut form_cluster_rx = Self::spawn_cluster_formation_delay_timer(self.config.clone()).fuse();
        loop {
            tokio::select! {
                Some(req) = self.requests.next() => self.handle_request(req).await,
                Some(forward) = self.forward_rx.next() => self.handle_forward_request(forward.0, forward.1).await,
                Some(metrics) = self.metrics.next() => {
                    tracing::trace!(?metrics, "raft metrics update"); // TODO: wire up metrics.
                }
                Ok(_) = &mut form_cluster_rx => self.initialize_raft_cluster().await,
                Some(needs_shutdown) = self.shutdown.next() => if needs_shutdown { break } else { continue },
            }
        }

        // Graceful shutdown. Here, we don't exit until all of our main channels have been drained.
        tracing::debug!("CRC is shutting down");
        while let Ok(req) = self.requests.try_recv() {
            self.handle_request(req).await;
        }
        while let Ok(forward) = self.forward_rx.try_recv() {
            self.handle_forward_request(forward.0, forward.1).await;
        }
        tracing::debug!("CRC has shutdown");
    }

    #[tracing::instrument(level = "trace", skip(self, req))]
    async fn handle_request(&mut self, req: CRCRequest) {
        match req {
            CRCRequest::Client(client_req) => {
                // Forward the request if needed.
                match self.raft.current_leader().await {
                    Some(leader) if leader == self.id => (), // This is the leader node, continue.
                    other_node_or_unknown => {
                        // A different node is the leader, so forward the request.
                        let _ = self.forward_tx.send((client_req, other_node_or_unknown));
                        return;
                    }
                }
                match client_req {
                    CRCClientRequest::UpdateSchema(req) => self.handle_request_update_schema(req).await,
                }
            }
            CRCRequest::Placement(req) => self.handle_cpc_stream_replica_assignment(req).await,
            CRCRequest::RaftAppendEntries(req) => self.handle_raft_append_entries_rpc(req).await,
            CRCRequest::RaftInstallSnapshot(req) => self.handle_raft_install_snapshot_rpc(req).await,
            CRCRequest::RaftVote(req) => self.handle_raft_vote_rpc(req).await,
        }
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

    pub(self) async fn get_peer_channel(&self, target: &u64) -> Result<Channel> {
        self.peer_channels
            .borrow()
            .get(&target)
            .cloned()
            .ok_or_else(|| anyhow!("no active connection to target peer"))
    }
}
