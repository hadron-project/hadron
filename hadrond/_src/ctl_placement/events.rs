//! CPC event code.

use std::sync::Arc;

use crate::models::placement::{ControlGroup, PipelineReplica, StreamReplica};

/// Events produced from the CPC.
pub enum CPCEvent {
    /// A stream replica has been scheduled on this node.
    StreamReplicaScheduled(Arc<StreamReplica>),
    /// A pipeline replica has been scheduled on this node.
    PipelineReplicaScheduled(Arc<PipelineReplica>),
}
