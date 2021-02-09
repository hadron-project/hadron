//! Cluster Placement Controller (CPC)
//!
//! This controller is responsible for tracking all live objects in the system which are in some
//! way distributed across the cluster. Streams, Pipelines, Endpoints and other such objects all
//! all have their own controllers which run on specific nodes of the cluster and work together
//! to accomplish their overall goal.
//!
//! This controller is responsible for determining exactly where on the cluster those controllers
//! are to run, and acts as a Raft client, sending various placement decision making requests to
//! CRC in order to ensure it is known and properly replicated across the cluster.
//!
//! This controller will only perform mutating actions when it is running on the same node as the
//! CRC leader (the Raft leader) — when it is the leader. Otherwise it will be completely
//! passive and never forwards placement commands to other nodes.

#![allow(unused_imports)] // TODO: remove this.
#![allow(unused_variables)] // TODO: remove this.
#![allow(unused_mut)] // TODO: remove this.
#![allow(dead_code)] // TODO: remove this.

pub mod models;

use std::collections::HashMap;
use std::sync::Arc;

use async_raft::RaftMetrics;
use models::StreamReplica;
use tokio::stream::StreamExt;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use crate::config::Config;
use crate::ctl_raft::events::{CRCEvent, InitialEvent, PipelineReplicaCreated, StreamReplicaCreated};
use crate::models as object_models;
use crate::models::Namespaced;
use crate::NodeId;

/// Cluster Placement Controller.
///
/// This controller is responsible for placement of objects & other controllers within the
/// cluster, and uses the Raft state machine as a command bus in order to do so.
///
/// When a CPC instance is on the leader node, it will assume the responsibility of issuing
/// commands to the rest of the cluster based on the state within the Raft state machine.
///
/// This ensures that we do not end up with nodes attempting to take actions on commands which are
/// indeed in the state machine, but which are old and which no longer apply. This could happen when
/// a node has become disconnected from the cluster but still has outstanding tasks to apply.
///
/// Instead, we use the approach of having the CPC leader issue commands. It passes along the Raft
/// term and its node ID, and other nodes simply verify that it is indeed the cluster leader. This
/// guards against any processing of stale commands as only the leader will process commands, and in
/// order to become leader, a node must be up-to-date.
pub struct CPC {
    /// The ID of this node in the cluster.
    id: NodeId,
    /// The application's runtime config.
    config: Arc<Config>,
    /// Application shutdown channel.
    shutdown: watch::Receiver<bool>,
    /// A handle to the Raft's metrics channel.
    metrics: watch::Receiver<RaftMetrics>,
    /// A handle to the Raft's metrics channel.
    crc_events: mpsc::UnboundedReceiver<CRCEvent>,
    /// A bool indicating if this node is currently the CPC leader, always matching the
    /// node's cluster Raft state.
    is_leader: bool,

    tasks_tx: mpsc::UnboundedSender<Task>,
    tasks_rx: mpsc::UnboundedReceiver<Task>,

    /// All nodes of the system along with their assignments.
    nodes: HashMap<NodeId, NodeAssignments>,
    /// A table of stream replicas, indexed by ID.
    stream_replicas: HashMap<u64, models::StreamReplica>,
    /// A table of pipeline replicas, indexed by ID.
    pipeline_replicas: HashMap<u64, models::PipelineReplica>,
    // TODO: endpoints & commands
}

impl CPC {
    pub fn new(
        id: NodeId, config: Arc<Config>, shutdown: watch::Receiver<bool>, metrics: watch::Receiver<RaftMetrics>,
        crc_events: mpsc::UnboundedReceiver<CRCEvent>,
    ) -> Self {
        let (tasks_tx, tasks_rx) = mpsc::unbounded_channel();
        let (nodes, stream_replicas, pipeline_replicas) = (Default::default(), Default::default(), Default::default());
        Self {
            id,
            config,
            shutdown,
            metrics,
            crc_events,
            nodes,
            is_leader: false,
            tasks_tx,
            tasks_rx,
            stream_replicas,
            pipeline_replicas,
        }
    }

    pub fn spawn(self) -> JoinHandle<()> {
        tokio::spawn(self.run())
    }

    async fn run(mut self) {
        tracing::debug!("Cluster Placement Controller is online");
        loop {
            tokio::select! {
                Some(update) = self.metrics.next() => self.handle_crc_raft_update(update).await,
                Some(event) = self.crc_events.next() => self.handle_crc_event(event).await,
                Some(task) = self.tasks_rx.next() => self.handle_reconciliation_task(task).await,
                Some(needs_shutdown) = self.shutdown.next() => if needs_shutdown { break } else { continue },
            }
        }

        // Graceful shutdown.
        tracing::debug!("cpc has shutdown");
    }

    /// Handle CRC metrics updates to drive leadership of this control group.
    #[tracing::instrument(level = "trace", skip(self, update))]
    async fn handle_crc_raft_update(&mut self, update: RaftMetrics) {
        // This node is currently operating as CPC leader, but is not on the Raft cluster leader
        // node, so step down and clear reconciliation tasks.
        if self.is_leader && !update.state.is_leader() {
            self.is_leader = false;
            while self.tasks_rx.try_recv().is_ok() {} // Drain.
            return;
        }
        // This node is currently not operating as CPC leader. Become leader and generate an
        // initial reconciliation task to kick things off.
        if !self.is_leader && update.state.is_leader() {
            let _ = self.tasks_tx.send(Task::Initial);
        }
    }

    /// Handle an event coming from the CRC.
    #[tracing::instrument(level = "trace", skip(self, event))]
    async fn handle_crc_event(&mut self, event: CRCEvent) {
        tracing::debug!("handling CRC event");
        match event {
            CRCEvent::Initial(inner) => self.handle_crc_initial(inner),
            CRCEvent::StreamReplicaCreated(event) => self.handle_crc_stream_replica_created(event),
            CRCEvent::PipelineReplicaCreated(event) => self.handle_crc_pipeline_replica_created(event),
            _ => (),
        }
    }

    /// Handle a reconciliation task generated from this controller or one of its spawned tasks.
    #[tracing::instrument(level = "trace", skip(self, task))]
    async fn handle_reconciliation_task(&mut self, task: Task) {
        tracing::debug!("handling CRC reconciliation task");
        // Ensure the inbound events queue is fully processed first.
        while let Some(event) = self.crc_events.next().await {
            self.handle_crc_event(event).await;
        }

        match task {
            Task::Initial => self.reconcile_initial().await,
        }
    }

    /// Perform an initial reconciliation pass.
    ///
    /// Review the state of all objects in memory, and spawn any reconciliation tasks needed.
    async fn reconcile_initial(&mut self) {
        // Find any streams which need placement.
        for (_, stream) in self.stream_replicas.iter() {
            if stream.current_node.is_none() && stream.aspired_node.is_none() {
                // TODO: this replica needs to be placed.
            }
        }

        /* NOTES: CRITICAL PATH >>>
        - when a replica has an aspired node, we must open a status channel to that node and wait
          until that replica has become a voting member of its raft. At that point, the CPC should
          transition its data state to being current on that node, and mark the old replica for removal.

        - cluster auto healing will require that nodes be cordoned when they are no longer appearing as
          healthy within the CRC cluster. When a node is cordoned by the CPC, it will not longer have
          objects scheduled on it. Once it passes the ClusterAutoHealingThreashold, it will then have
          all of its active controllers migrated off of the node, and then marked for removal.
        - once a CRC node has been marked for removal, the CPC will issue a client request to the CRC
          Raft to have that node removed from the cluster.
        - new nodes coming in from the discovery system will be added to the CRC Raft by the CPC.
            - probably update the handshake system to ensure that nodes are part of the same cluster.
            - update Hadron config to take a cluster name. This will ensure disparate clusters are not
              unintentionally joined. Connection can just be dropped at network layer.

        - ultimately, we will need a reconciler check to compare objects currently on the node vs
          objects which are supposed to be, so that old objects can be removed.
        */
    }

    /// Handle the initial payload of data from the CRC after system restore.
    ///
    /// This will cause all data held by this system to be purged and recreated.
    #[tracing::instrument(level = "trace", skip(self, initial))]
    fn handle_crc_initial(&mut self, initial: InitialEvent) {
        tracing::debug!("handling initial event from CRC");

        // Update stream placements.
        self.stream_replicas.clear();
        for placement in initial.stream_replicas {
            if let Some(node) = placement.current_node {
                let assignments = self.nodes.entry(node).or_insert_with(|| NodeAssignments::new(node));
                assignments.total_assignments += 1;
                assignments.stream_replicas.insert(placement.id, placement.clone());
            }
            if let Some(node) = placement.aspired_node {
                let assignments = self.nodes.entry(node).or_insert_with(|| NodeAssignments::new(node));
                assignments.total_assignments += 1;
                assignments.stream_replicas.insert(placement.id, placement.clone());
            }
            self.stream_replicas.insert(placement.id, placement);
        }

        // Update pipeline assignments.
        self.pipeline_replicas.clear();
        for placement in initial.pipeline_replicas {
            if let Some(node) = placement.current_node {
                let assignments = self.nodes.entry(node).or_insert_with(|| NodeAssignments::new(node));
                assignments.total_assignments += 1;
                assignments.pipeline_replicas.insert(placement.id, placement.clone());
            }
            if let Some(node) = placement.aspired_node {
                let assignments = self.nodes.entry(node).or_insert_with(|| NodeAssignments::new(node));
                assignments.total_assignments += 1;
                assignments.pipeline_replicas.insert(placement.id, placement.clone());
            }
            self.pipeline_replicas.insert(placement.id, placement);
        }
    }

    /// Handle CRC event for a newly created stream replica.
    #[tracing::instrument(level = "trace", skip(self, event))]
    fn handle_crc_stream_replica_created(&mut self, event: StreamReplicaCreated) {
        tracing::debug!("handling CRC stream replica created event");
        // Find all nodes which are not running a replica for this replica's partition.

        // Of those nodes, select the one with least load.
    }

    /// Handle CRC event for a newly created pipeline replica.
    #[tracing::instrument(level = "trace", skip(self, event))]
    fn handle_crc_pipeline_replica_created(&mut self, event: PipelineReplicaCreated) {
        tracing::debug!("handling CRC pipeline replica created event");
        // TODO: same as stream placement.
    }
}

/// Details on all assignments for a specific node of the cluster.
struct NodeAssignments {
    /// The ID of the corresponding node.
    pub id: NodeId,
    /// The total number of assignments on this node.
    pub total_assignments: u64,
    /// All stream replicas which are currently assigned to this node.
    pub stream_replicas: HashMap<u64, models::StreamReplica>,
    /// All pipeline replicas which are currently assigned to this node.
    pub pipeline_replicas: HashMap<u64, models::PipelineReplica>,
}

impl NodeAssignments {
    /// Create a new instance.
    pub fn new(id: NodeId) -> Self {
        Self {
            id,
            total_assignments: 0,
            stream_replicas: Default::default(),
            pipeline_replicas: Default::default(),
        }
    }
}

/// A reconciliation task used by the CPC.
enum Task {
    Initial,
}
