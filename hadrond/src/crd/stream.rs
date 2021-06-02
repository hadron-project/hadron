//! Stream CRD.
//!
//! The code here is used to generate the actual CRD used in K8s. See examples/crd.rs.

use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// CRD spec for the Stream resource.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, CustomResource, JsonSchema)]
#[kube(
    struct = "Stream",
    status = "StreamStatus",
    group = "hadron.rs",
    version = "v1",
    kind = "Stream",
    namespaced,
    derive = "PartialEq",
    apiextensions = "v1",
    shortname = "stream",
    printcolumn = r#"{"name":"Cluster","type":"string","jsonPath":".spec.cluster"}"#,
    printcolumn = r#"{"name":"Partitions","type":"number","jsonPath":".spec.partitions"}"#,
    printcolumn = r#"{"name":"Replicas","type":"number","jsonPath":".spec.replicas"}"#,
    printcolumn = r#"{"name":"TTL","type":"number","jsonPath":".spec.ttl"}"#
)]
pub struct StreamSpec {
    /// The cluster to which this token belongs.
    pub cluster: String,
    /// The number of partitions to be created for this stream.
    pub partitions: u8,
    /// The number of replicas to be used per partition.
    pub replicas: u8,
    /// An optional TTL in seconds specifying how long records are to be kept on the stream.
    ///
    /// If `0`, then records will stay on the stream forever.
    #[serde(default)]
    pub ttl: u64,
}

/// CRD status object.
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, JsonSchema)]
pub struct StreamStatus {
    /// The serial epoch of this partition assignment configuration.
    pub epoch: u64,
    /// State info on all partitions.
    pub partitions: Vec<Partition>,
}

/// State information of a partition.
///
/// ## RuntimeState
/// Stable:
/// - all assigned pods are running and healthy.
///
/// Unstable:
/// - at least one backing pod is unhealthy or has been deleted.
///
/// ## ScheduleState
/// Scheduled:
/// - a leader has been assigned.
/// - the last known target replica count has been matched.
///
/// Unschedulable:
/// - a leader could not be assigned.
/// - the last known target replica count could not be matched.
///
/// NeedsReplicas:
/// - a leader has been assigned.
/// - some replicas have been assigned, but the last known target replica count could not be matched.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, JsonSchema)]
pub struct Partition {
    /// The partition's schedule state.
    #[serde(rename = "scheduleState")]
    pub schedule_state: ScheduleState,
    /// The partition's runtime state.
    #[serde(rename = "runtimeState")]
    pub runtime_state: RuntimeState,
    /// The name of the pod which is currently the leader of the partition.
    leader: String,
    /// The names of all pods acting as replicas for the partition.
    replicas: Vec<String>,
}

/// Scheduling state of a partition.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, JsonSchema)]
pub enum ScheduleState {
    /// A leader has been assigned and the last known target
    /// replica count has been matched.
    Scheduled,
    /// A leader could not be assigned and the last known target
    /// replica count could not be matched.
    Unschedulable,
    /// A leader and potentially some replicas were previously assigned,
    /// but the last known target replica count could not be matched.
    NeedsReplicas,
}

/// Runtime state of a partition.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, JsonSchema)]
pub enum RuntimeState {
    /// All partitions pods are running and healthy.
    Stable,
    /// At least one partition pod is unhealthy or has been deleted.
    Unstable,
    /// The state of the backing pods assigned to this partition is not yet known.
    Unknown,
}

impl Partition {
    /// Create a new partition in an unschedulable state.
    pub fn new(schedule_state: ScheduleState, runtime_state: RuntimeState, leader: Option<String>, replicas: Option<Vec<String>>) -> Self {
        Self {
            schedule_state,
            runtime_state,
            leader: leader.unwrap_or_default(),
            replicas: replicas.unwrap_or_default(),
        }
    }

    /// The pod name of the leader of this partition if scheduled, else `None` if not scheduled.
    pub fn leader(&self) -> Option<&String> {
        match &self.schedule_state {
            ScheduleState::Scheduled => Some(&self.leader),
            ScheduleState::Unschedulable => None,
            ScheduleState::NeedsReplicas => Some(&self.leader),
        }
    }

    /// The replicas of this partition if scheduled, else `None` if not scheduled.
    pub fn replicas(&self) -> Option<&[String]> {
        match &self.schedule_state {
            ScheduleState::Scheduled => Some(self.replicas.as_slice()),
            ScheduleState::Unschedulable => None,
            ScheduleState::NeedsReplicas => Some(self.replicas.as_slice()),
        }
    }

    /// Add a replica to the partition.
    pub fn add_replica(&mut self, pod: String) {
        if matches!(self.schedule_state, ScheduleState::Unschedulable) {
            return;
        }
        self.replicas.push(pod);
    }

    /// Pop a replica from the end of the partition's replicas vec.
    pub fn pop_replica(&mut self) -> Option<String> {
        if matches!(self.schedule_state, ScheduleState::Unschedulable) {
            return None;
        }
        self.replicas.pop()
    }

    /// Check if this partition has been scheduled at any capacity
    /// (E.G., a schedule state other than Unschedulable).
    pub fn is_scheduled(&self) -> bool {
        !matches!(self.schedule_state, ScheduleState::Unschedulable)
    }
}

impl Stream {
    /// Get the partition number for which the given pod is a leader, else `None`.
    #[allow(clippy::ptr_arg)]
    pub fn get_pod_leader_partition(&self, pod: &String) -> Option<u8> {
        self.status.as_ref().and_then(|status| {
            status
                .partitions
                .as_slice()
                .iter()
                .enumerate()
                .filter(|(_, prtn)| prtn.is_scheduled())
                .find(|(_, prtn)| &prtn.leader == pod)
                .map(|(offset, _)| offset as u8)
        })
    }

    /// Check the given partition to see if it currently has the given pod as a scheduled replica.
    #[allow(clippy::ptr_arg)]
    pub fn has_pod_as_partition_replica(&self, pod: &String, partition: u8) -> bool {
        self.status
            .as_ref()
            .and_then(|status| {
                status
                    .partitions
                    .as_slice()
                    .iter()
                    .nth(partition as usize)
                    .map(|prtn| prtn.replicas.contains(pod))
            })
            .unwrap_or(false)
    }

    /// Get an iterator over all partition offsets of this stream which have the given pod scheduled as a replica.
    #[allow(clippy::ptr_arg)]
    pub fn scheduled_replica_partitions<'a>(&'a self, pod: &'a String) -> Option<impl Iterator<Item = u8> + 'a> {
        self.status.as_ref().map(move |status| {
            status
                .partitions
                .as_slice()
                .iter()
                .enumerate()
                .filter(|(_, prtn)| prtn.is_scheduled())
                .filter(move |(_, prtn)| prtn.replicas.contains(pod))
                .map(|(offset, _)| offset as u8)
        })
    }
}
