// //! Data models for the CPC.

// use serde::{Deserialize, Serialize};

// use crate::NodeId;

// /// Assignment variants which can be issued from the CPC.
// #[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
// pub enum Assignment {
//     /// An assignment for an object which was in pristine state, not having a previous assignment.
//     InitialPlacement {
//         /// The target node of the initial placement.
//         node: NodeId,
//     },
// }

// /// A stream partition replica.
// ///
// /// Each replica is uniquely identified via the compound key
// /// (namespace, stream, partition, replica). On disk, these models are keyed such that a prefix
// /// scan can be used to look-up all replicas by stream, and or by partition.
// #[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
// pub struct StreamReplica {
//     /// The ID of this object.
//     pub id: u64,
//     /// The ID of this object's control group.
//     pub cg_id: u64,
//     /// The ID of the stream to which this replica belongs.
//     pub stream_id: u64,
//     /// The partition to which this replica belongs.
//     pub partition: u32,
//     /// The offset of this replica.
//     pub replica: u8,
//     /// The ID of the node on which this replica currently resides.
//     pub current_node: Option<NodeId>,
//     /// The ID of the node to which this replica needs to be moved.
//     pub aspired_node: Option<NodeId>,
// }

// impl StreamReplica {
//     /// Create a new instance.
//     pub fn new(id: u64, cg_id: u64, stream_id: u64, partition: u32, replica: u8) -> Self {
//         Self {
//             id,
//             cg_id,
//             stream_id,
//             partition,
//             replica,
//             current_node: None,
//             aspired_node: None,
//         }
//     }
// }

// /// A pipeline replica.
// ///
// /// Each replica is uniquely identified via the compound key
// /// (namespace, pipeline, replica). On disk, these models are keyed such that a prefix
// /// scan can be used to look-up all replicas by pipeline.
// #[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
// pub struct PipelineReplica {
//     /// The unique ID of this object.
//     pub id: u64,
//     /// The ID of this object's control group.
//     pub cg_id: u64,
//     /// The ID of the pipeline to which this replica belongs.
//     pub stream_id: u64,
//     /// The offset of this replica.
//     pub replica: u8,
//     /// The ID of the node on which this replica currently resides.
//     pub current_node: Option<NodeId>,
//     /// The ID of the node to which this replica needs to be moved.
//     pub aspired_node: Option<NodeId>,
// }

// impl PipelineReplica {
//     /// Create a new instance.
//     pub fn new(id: u64, cg_id: u64, stream_id: u64, replica: u8) -> Self {
//         Self {
//             id,
//             cg_id,
//             stream_id,
//             replica,
//             current_node: None,
//             aspired_node: None,
//         }
//     }
// }

// /// A group of controllers working together via the ISR protocol.
// #[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
// pub struct ControlGroup {
//     /// The ID of this object.
//     pub id: u64,
//     /// The object to which this control group is associated.
//     pub object_ref: ControlGroupObjectRef,
//     /// The ID of all replicas which are part of this group.
//     pub replicas: Vec<u64>,
//     /// The ID of all cluster nodes which are part of this group.
//     pub nodes: Vec<NodeId>,
//     /// All cluster nodes which are in-sync with the group leader.
//     pub isr: Vec<NodeId>,
//     /// The ID of the cluster node which is the current group leader, along with its term.
//     ///
//     /// A cluster node may only participate in a CG once, though a replica of the group may have
//     /// a running copy on multiple nodes for cases of placement updates where a replica is being
//     /// moved from one node to another.
//     ///
//     /// The term is a monotonically increasing value.
//     pub leader: Option<(NodeId, u64)>,
// }

// impl ControlGroup {
//     /// Create a new instance for a pipeline.
//     pub fn new_stream(id: u64, partition: u32, replicas: Vec<u64>) -> Self {
//         Self {
//             id,
//             object_ref: ControlGroupObjectRef::Stream { id, partition },
//             replicas,
//             nodes: vec![],
//             isr: vec![],
//             leader: None,
//         }
//     }
//     /// Create a new instance for a pipeline.
//     pub fn new_pipeline(id: u64, replicas: Vec<u64>) -> Self {
//         Self {
//             id,
//             object_ref: ControlGroupObjectRef::Pipeline { id },
//             replicas,
//             nodes: vec![],
//             isr: vec![],
//             leader: None,
//         }
//     }
// }

// /// A reference to a control group's associated object.
// #[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
// #[serde(tag = "object")]
// pub enum ControlGroupObjectRef {
//     Stream {
//         /// The ID of the associated stream.
//         id: u64,
//         /// The partition number of the associated stream.
//         partition: u32,
//     },
//     Pipeline {
//         /// The ID of the associated pipeline.
//         id: u64,
//     },
// }
