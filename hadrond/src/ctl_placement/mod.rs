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

pub mod events;
mod tasks;

use std::collections::HashMap;
use std::sync::Arc;

use async_raft::RaftMetrics;
use tokio::stream::StreamExt;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use crate::config::Config;
use crate::ctl_raft::events::{CRCEvent, InitialEvent, StreamReplicaAssignmentUpdated};
use crate::ctl_raft::models::CRCRequest;
use crate::models::placement::{Assignment, PipelineReplica, StreamReplica};
use crate::models::schema::Namespaced;
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
    /// An output channel directly to the CRC.
    crc_tx: mpsc::UnboundedSender<CRCRequest>,
    /// A channel of messages outbound from this controller.
    cpc_tx: mpsc::UnboundedSender<events::CPCEvent>,
    /// A bool indicating if this node is currently the CPC leader, always matching the
    /// node's cluster Raft state.
    is_leader: bool,

    tasks_tx: mpsc::UnboundedSender<Task>,
    tasks_rx: mpsc::UnboundedReceiver<Task>,

    /// All nodes of the system along with their assignments.
    nodes: ClusterNodes,
    /// A table of stream replicas, indexed by ID.
    stream_replicas: HashMap<u64, Arc<StreamReplica>>,
    /// A wait list of stream replicas which were not able to be scheduled.
    wait_list_streams: HashMap<u64, Arc<StreamReplica>>,
    /// A table of pipeline replicas, indexed by ID.
    pipeline_replicas: HashMap<u64, Arc<PipelineReplica>>,
    /// A wait list of pipeline replicas which were not able to be scheduled.
    wait_list_pipelines: HashMap<u64, Arc<PipelineReplica>>,
    // TODO: endpoints & commands
}

impl CPC {
    pub fn new(
        id: NodeId, config: Arc<Config>, shutdown: watch::Receiver<bool>, metrics: watch::Receiver<RaftMetrics>,
        crc_events: mpsc::UnboundedReceiver<CRCEvent>, crc_tx: mpsc::UnboundedSender<CRCRequest>,
    ) -> (Self, mpsc::UnboundedReceiver<events::CPCEvent>) {
        let (tasks_tx, tasks_rx) = mpsc::unbounded_channel();
        let (wait_list_streams, wait_list_pipelines, stream_replicas, pipeline_replicas) =
            (Default::default(), Default::default(), Default::default(), Default::default());
        let (cpc_tx, cpc_rx) = mpsc::unbounded_channel();
        let nodes = ClusterNodes::new(id, metrics.borrow().membership_config.members.iter());
        let this = Self {
            id,
            config,
            shutdown,
            metrics,
            crc_events,
            nodes,
            crc_tx,
            cpc_tx,
            is_leader: false,
            tasks_tx,
            tasks_rx,
            stream_replicas,
            wait_list_streams,
            pipeline_replicas,
            wait_list_pipelines,
        };
        (this, cpc_rx)
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
            CRCEvent::StreamReplicaCreated(replica) => self.handle_crc_stream_replica_created(replica),
            CRCEvent::StreamReplicaAssignmentUpdated(event) => self.handle_crc_stream_replica_assignment_update(event),
            CRCEvent::PipelineReplicaCreated(replica) => self.handle_crc_pipeline_replica_created(replica),
            _ => (),
        }
    }

    /// Handle a reconciliation task generated from this controller or one of its spawned tasks.
    #[tracing::instrument(level = "trace", skip(self, task))]
    async fn handle_reconciliation_task(&mut self, task: Task) {
        tracing::debug!("handling CRC reconciliation task");
        // Ensure the inbound events queue is fully processed first.
        while let Ok(event) = self.crc_events.try_recv() {
            self.handle_crc_event(event).await;
        }

        match task {
            Task::Initial => self.reconcile_initial().await,
            Task::StreamReplicaCreated(replica) => self.reconcile_stream_replica_created(replica).await,
        }
    }

    /// Perform an initial reconciliation pass.
    ///
    /// Review the state of all objects in memory, and spawn any reconciliation tasks needed.
    #[tracing::instrument(level = "trace", skip(self))]
    async fn reconcile_initial(&mut self) {
        tracing::trace!(stream_replicas = ?self.stream_replicas, "performing full CPC reconciliation");

        // Ensure our cluster node info is up-to-date.
        self.nodes.update_nodes(self.metrics.borrow().membership_config.members.iter());

        // Find any stream replicas which need initial placement.
        let mut stream_replicas_to_place = self
            .stream_replicas
            .iter()
            .filter(|(_, stream)| stream.current_node.is_none() && stream.aspired_node.is_none());
        for (_, replica) in stream_replicas_to_place {
            let node = match self.nodes.assign_stream_replica(replica.clone()) {
                Some(node) => node,
                None => {
                    // The replica could not be scheduled for various reasons, so add it to the wait list.
                    self.wait_list_streams.insert(replica.id, replica.clone());
                    continue;
                }
            };
            tokio::spawn(tasks::assign_stream_replica_to_node(
                Assignment::InitialPlacement { node },
                replica.clone(),
                self.crc_tx.clone(),
                self.tasks_tx.clone(),
            ));
        }

        tracing::trace!(waitlist = ?self.wait_list_streams);
        tracing::trace!(assignments = ?self.nodes);

        // Find any stream replicas with an aspired node, and drive them to completion.
        // TODO: ^^^

        // Find any pipeline replicas which need initial placement.
        let pipeline_replicas_to_place = self
            .pipeline_replicas
            .iter()
            .filter(|(_, pipeline)| pipeline.current_node.is_none() && pipeline.aspired_node.is_none());

        // Find any pipeline replicas with an aspired node, and drive them to completion.
        // TODO: ^^^

        /* TODO:NOTES:
        - should we actually just make this a stream controller so that all other stream controllers
          and various objects can subscribe to the events it creates? Placement info, cluster membership,
          and various other details could be propagated to other controllers. Probably not. Let's use
          a different model for propagating info local to a node.

        - when a replica has an aspired node, we must open a status channel to that node and wait
          until that replica has become a voting member of its raft. At that point, the CPC should
          transition its data state to being current on that node. If there was an old replica, the
          CPC on that old node will automatically clean up the old replica when its CRC is updated.

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

        // Compare local stream replicas on disk vs streams which need to exist,
        // and then spawn and delete controllers as needed.
        // TODO: ^^^

        // Compare local pipeline replicas on disk vs streams which need to exist,
        // and then spawn and delete controllers as needed.
        // TODO: ^^^
    }

    /// Reconcile initial creation of a stream replica.
    #[tracing::instrument(level = "trace", skip(self, replica))]
    async fn reconcile_stream_replica_created(&mut self, replica: Arc<StreamReplica>) {
        let node = match self.nodes.assign_stream_replica(replica.clone()) {
            Some(node) => node,
            None => {
                // The replica could not be scheduled for various reasons, so add it to the wait list.
                self.wait_list_streams.insert(replica.id, replica.clone());
                return;
            }
        };
        tokio::spawn(tasks::assign_stream_replica_to_node(
            Assignment::InitialPlacement { node },
            replica,
            self.crc_tx.clone(),
            self.tasks_tx.clone(),
        ));
    }

    /// Handle the initial payload of data from the CRC after system restore.
    ///
    /// This will cause all data held by this system to be purged and recreated.
    #[tracing::instrument(level = "trace", skip(self, initial))]
    fn handle_crc_initial(&mut self, initial: InitialEvent) {
        tracing::debug!("handling initial event from CRC");

        // Clear cluster node assignment info.
        self.nodes.clear();

        // Update stream placements.
        self.stream_replicas.clear();
        for placement in initial.stream_replicas {
            self.stream_replicas.insert(placement.id, placement.clone());
            if let Some(event) = self.nodes.record_stream_replica(placement) {
                let _ = self.cpc_tx.send(event);
            }
        }

        // Update pipeline assignments.
        self.pipeline_replicas.clear();
        for placement in initial.pipeline_replicas {
            self.pipeline_replicas.insert(placement.id, placement.clone());
            if let Some(event) = self.nodes.record_pipeline_replica(placement) {
                let _ = self.cpc_tx.send(event);
            }
        }
    }

    /// Handle CRC event for a newly created stream replica.
    ///
    /// All we do here is update this replica's data in the index & create a
    /// reconciliation task if we are on the leader node.
    #[tracing::instrument(level = "trace", skip(self, replica))]
    fn handle_crc_stream_replica_created(&mut self, replica: Arc<StreamReplica>) {
        tracing::debug!("handling CRC stream replica created event");
        self.stream_replicas.insert(replica.id, replica.clone());
        if self.is_leader {
            let _ = self.tasks_tx.send(Task::StreamReplicaCreated(replica));
        }
    }

    /// Handle CRC event for a newly created pipeline replica.
    ///
    /// All we do here is update this replica's data in the index & create a
    /// reconciliation task if we are on the leader node.
    #[tracing::instrument(level = "trace", skip(self, replica))]
    fn handle_crc_pipeline_replica_created(&mut self, replica: Arc<PipelineReplica>) {
        tracing::debug!("handling CRC pipeline replica created event");
        // TODO: add data to system & if leader, add a reconcilition task places replica.
    }

    /// Handle CRC event for a newly created pipeline replica.
    #[tracing::instrument(level = "trace", skip(self, event))]
    fn handle_crc_stream_replica_assignment_update(&mut self, event: StreamReplicaAssignmentUpdated) {
        tracing::debug!("handling CRC stream replica assignment update event");
        // Update this replica's data in the index.
        self.stream_replicas.insert(event.replica.id, event.replica.clone());

        // Update node assignment info.
        if let Some(event) = self.nodes.update_stream_replica_assignment(event.change, event.replica) {
            let _ = self.cpc_tx.send(event);
        }
    }
}

/// A representation of all known cluster memebers and their assignments.
#[derive(Debug)]
struct ClusterNodes {
    /// The ID of this node within the cluster.
    id: NodeId,
    /// All known nodes within the cluster along with their known assignments.
    pub nodes: HashMap<NodeId, NodeAssignments>,
    /// A sorted load order of all nodes in the cluster.
    load_order: Vec<(NodeId, u64)>,
}

impl ClusterNodes {
    /// Create a new instance.
    pub fn new<'a>(id: NodeId, nodes: impl Iterator<Item = &'a NodeId>) -> Self {
        let nodes = nodes.fold(HashMap::new(), |mut acc, id| {
            acc.insert(*id, NodeAssignments::new(*id));
            acc
        });
        let load_order = nodes.keys().map(|id| (*id, 0)).collect();
        Self { id, nodes, load_order }
    }

    /// Ensure the system is up-to-date with all cluster nodes.
    pub fn update_nodes<'a>(&mut self, nodes: impl Iterator<Item = &'a NodeId>) {
        for node in nodes {
            if !self.nodes.contains_key(node) {
                self.nodes.insert(*node, NodeAssignments::new(*node));
                self.load_order.push((*node, 0));
                self.sort_load_order();
            }
        }
    }

    /// Assign a stream replica to a node.
    ///
    /// This will update the pending assignments for the scheduled node, and will add the replica
    /// to its table of stream replicas. Once the task has been driven through to completion or it
    /// fails, this assignment will need to be updated.
    fn assign_stream_replica(&mut self, replica: Arc<StreamReplica>) -> Option<NodeId> {
        let mut assigned_node = None;
        for (id, assignments) in self.load_order.iter_mut() {
            let node = match self.nodes.get_mut(id) {
                Some(assignments) => assignments,
                None => continue,
            };
            let has_partition_repl = node
                .stream_replicas
                .values()
                .any(|repl| repl.namespace == replica.namespace && repl.name == replica.name && repl.partition == replica.partition);
            if has_partition_repl {
                continue;
            }
            // This node is the first node with the least load which does not already have a workload
            // scheduled for the same stream partition. Schedule on this node.
            node.pending_assignments += 1;
            node.stream_replicas.insert(replica.id, replica);
            *assignments += 1;
            assigned_node = Some(*id);
            break;
        }

        // If we have an assigned node, update sorting of our load order vec.
        if assigned_node.is_some() {
            self.sort_load_order();
        }
        assigned_node
    }

    /// Clear all info on this object, resetting back to a pristine state.
    fn clear(&mut self) {
        self.nodes.clear();
        self.load_order.clear();
    }

    /// Record data on a stream replica assignment based on received data.
    ///
    /// This will only update the state of the system if the replica has a current or aspired
    /// node assignment.
    fn record_stream_replica(&mut self, replica: Arc<StreamReplica>) -> Option<events::CPCEvent> {
        let mut event = None;
        if let Some(node) = replica.current_node {
            let assignments = self.nodes.entry(node).or_insert_with(|| NodeAssignments::new(node));
            assignments.total_assignments += 1;
            assignments.stream_replicas.insert(replica.id, replica.clone());
            match self.load_order.iter_mut().find(|n| n.0 == node) {
                Some((_, val)) => *val += 1,
                None => self.load_order.push((node, 1)),
            }
            if node == self.id {
                event = Some(events::CPCEvent::StreamReplicaScheduled(replica.clone()))
            }
        }
        if let Some(node) = replica.aspired_node {
            let assignments = self.nodes.entry(node).or_insert_with(|| NodeAssignments::new(node));
            assignments.total_assignments += 1;
            assignments.stream_replicas.insert(replica.id, replica.clone());
            match self.load_order.iter_mut().find(|n| n.0 == node) {
                Some((_, val)) => *val += 1,
                None => self.load_order.push((node, 1)),
            }
            if node == self.id {
                event = Some(events::CPCEvent::StreamReplicaScheduled(replica))
            }
        }
        self.sort_load_order();
        event
    }

    /// Record data on a stream replica assignment based on received data.
    ///
    /// This will only update the state of the system if the replica has a current or aspired
    /// node assignment.
    fn record_pipeline_replica(&mut self, replica: Arc<PipelineReplica>) -> Option<events::CPCEvent> {
        let mut event = None;
        if let Some(node) = replica.current_node {
            let assignments = self.nodes.entry(node).or_insert_with(|| NodeAssignments::new(node));
            assignments.total_assignments += 1;
            assignments.pipeline_replicas.insert(replica.id, replica.clone());
            match self.load_order.iter_mut().find(|n| n.0 == node) {
                Some((_, val)) => *val += 1,
                None => self.load_order.push((node, 1)),
            }
            if node == self.id {
                event = Some(events::CPCEvent::PipelineReplicaScheduled(replica.clone()))
            }
        }
        if let Some(node) = replica.aspired_node {
            let assignments = self.nodes.entry(node).or_insert_with(|| NodeAssignments::new(node));
            assignments.total_assignments += 1;
            assignments.pipeline_replicas.insert(replica.id, replica.clone());
            match self.load_order.iter_mut().find(|n| n.0 == node) {
                Some((_, val)) => *val += 1,
                None => self.load_order.push((node, 1)),
            }
            if node == self.id {
                event = Some(events::CPCEvent::PipelineReplicaScheduled(replica))
            }
        }
        self.sort_load_order();
        event
    }

    /// Handle an update to a stream replica assignment.
    fn update_stream_replica_assignment(&mut self, change: Assignment, replica: Arc<StreamReplica>) -> Option<events::CPCEvent> {
        match change {
            // The replica was in a pristine state and has now been assigned to a node.
            Assignment::InitialPlacement { node: node_id } => {
                let node = self.nodes.entry(node_id).or_insert_with(|| NodeAssignments::new(node_id));
                node.total_assignments += 1;
                node.stream_replicas.insert(replica.id, replica.clone());
                match self.load_order.iter_mut().find(|(node, _)| node == &node_id) {
                    Some((_, load)) => *load += 1,
                    None => self.load_order.push((node_id, 1)),
                }
                self.sort_load_order();

                // If the assignment was for this node, then emit a scheduling event.
                if node_id == self.id {
                    Some(events::CPCEvent::StreamReplicaScheduled(replica))
                } else {
                    None
                }
            }
        }
    }

    /// Sort the values in load order.
    ///
    /// This will place more heavily loaded nodes at the end of the vector.
    fn sort_load_order(&mut self) {
        self.load_order.sort_by(|a, b| a.1.cmp(&b.1))
    }
}

/// Details on all assignments for a specific node of the cluster.
#[derive(Debug)]
struct NodeAssignments {
    /// The ID of the corresponding node.
    pub id: NodeId,
    /// The total number of assignments on this node.
    pub total_assignments: u64,
    /// The total number of pending assignments being made.
    pub pending_assignments: u64,
    /// All stream replicas which are currently assigned to this node.
    pub stream_replicas: HashMap<u64, Arc<StreamReplica>>,
    /// All pipeline replicas which are currently assigned to this node.
    pub pipeline_replicas: HashMap<u64, Arc<PipelineReplica>>,
}

impl NodeAssignments {
    /// Create a new instance.
    pub fn new(id: NodeId) -> Self {
        Self {
            id,
            total_assignments: 0,
            pending_assignments: 0,
            stream_replicas: Default::default(),
            pipeline_replicas: Default::default(),
        }
    }
}

/// A reconciliation task used by the CPC.
enum Task {
    Initial,
    StreamReplicaCreated(Arc<StreamReplica>),
}
