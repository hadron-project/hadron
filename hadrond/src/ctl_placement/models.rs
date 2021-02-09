//! Data models for the CPC.

use serde::{Deserialize, Serialize};

use crate::NodeId;

/// A stream partition replica.
///
/// Each replica is uniquely identified via the compound key
/// (namespace, stream, partition, replica). On disk, these models are keyed such that a prefix
/// scan can be used to look-up all replicas by stream, and or by partition.
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct StreamReplica {
    /// The unique ID of this object.
    pub id: u64,
    /// The namespace of the stream to which this replica belongs.
    pub namespace: String,
    /// The name of the stream to which this replica belongs.
    pub name: String,
    /// The partition to which this replica belongs.
    pub partition: u32,
    /// The offset of this replica.
    pub replica: u8,
    /// The ID of the node on which this replica currently resides.
    pub current_node: Option<NodeId>,
    /// The ID of the node to which this replica needs to be moved.
    pub aspired_node: Option<NodeId>,
}

impl StreamReplica {
    /// Create a new instance.
    pub fn new(id: u64, namespace: &str, name: &str, partition: u32, replica: u8) -> Self {
        Self {
            id,
            namespace: namespace.into(),
            name: name.into(),
            partition,
            replica,
            current_node: None,
            aspired_node: None,
        }
    }
}

/// A pipeline replica.
///
/// Each replica is uniquely identified via the compound key
/// (namespace, pipeline, replica). On disk, these models are keyed such that a prefix
/// scan can be used to look-up all replicas by pipeline.
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
pub struct PipelineReplica {
    /// The unique ID of this object.
    pub id: u64,
    /// The namespace of the pipeline to which this replica belongs.
    pub namespace: String,
    /// The name of the pipeline to which this replica belongs.
    pub name: String,
    /// The offset of this replica.
    pub replica: u8,
    /// The ID of the node on which this replica currently resides.
    pub current_node: Option<NodeId>,
    /// The ID of the node to which this replica needs to be moved.
    pub aspired_node: Option<NodeId>,
}

impl PipelineReplica {
    /// Create a new instance.
    pub fn new(id: u64, namespace: &str, name: &str, replica: u8) -> Self {
        Self {
            id,
            namespace: namespace.into(),
            name: name.into(),
            replica,
            current_node: None,
            aspired_node: None,
        }
    }
}

// /// A CPC placement command.
// #[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
// pub struct PlacementCommand {
//     pub id: u64,
//     pub command: PlacementCommandType,
// }

// /// A CPC placement command type.
// #[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
// pub enum PlacementCommandType {
//     AssignStreamReplica(AssignStreamReplica),
// }

// /// A placement command to assign a stream replica to a specific node of the cluster.
// ///
// /// The CPC will communicate with the CPC of the target node and will ensure that a
// #[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
// pub struct AssignStreamReplica {
// }
