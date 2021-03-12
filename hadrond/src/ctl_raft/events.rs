//! CRC application commands.
//!
//! The CRC is responsible for controlling the state of the Hadron cluster as a whole, and exposes
//! a control signal to the application in order to drive the system. That logic is encapsulated here.

use std::sync::Arc;

use crate::models::placement::{Assignment, ControlGroup, PipelineReplica, StreamReplica};
use crate::models::schema::{Pipeline, Stream};
use crate::models::WithId;

/// An event coming from the CRC.
pub enum CRCEvent {
    /// An initial payload of data from the CRC.
    Initial(InitialEvent),
    /// An event indicating that a new stream was created.
    StreamCreated(StreamCreated),
    /// An event indicating that a new stream replica was created.
    StreamReplicaCreated(Arc<StreamReplica>),
    /// An event indicating that a new stream replica was created.
    StreamReplicaAssignmentUpdated(StreamReplicaAssignmentUpdated),
    /// An event indicating that a new pipeline was created.
    PipelineCreated(PipelineCreated),
    /// An event indicating that a new pipeline replica was created.
    PipelineReplicaCreated(Arc<PipelineReplica>),
}

/// An initial payload of data from the CRC.
///
/// This event will be emitted from the CRC when it is first initialized after recovering its
/// state from storage.
pub struct InitialEvent {
    /// All known streams in the cluster.
    pub streams: Vec<Arc<WithId<Stream>>>,
    /// All known stream replicas in the cluster.
    pub stream_replicas: Vec<Arc<StreamReplica>>,
    /// All known pipelines in the cluster.
    pub pipelines: Vec<Arc<WithId<Pipeline>>>,
    /// All known pipeline replicas in the cluster.
    pub pipeline_replicas: Vec<Arc<PipelineReplica>>,
    /// All known control groups in the cluster.
    pub control_groups: Vec<Arc<ControlGroup>>,
}

impl InitialEvent {
    /// Create a new instance.
    pub fn new(
        streams: Vec<Arc<WithId<Stream>>>, stream_replicas: Vec<Arc<StreamReplica>>, pipelines: Vec<Arc<WithId<Pipeline>>>,
        pipeline_replicas: Vec<Arc<PipelineReplica>>, control_groups: Vec<Arc<ControlGroup>>,
    ) -> Self {
        Self {
            streams,
            stream_replicas,
            pipelines,
            pipeline_replicas,
            control_groups,
        }
    }
}

/// An event indicating that a new stream was created.
pub struct StreamCreated {
    /// The stream's data model.
    pub stream: Arc<WithId<Stream>>,
    /// The replicas associated with this stream.
    pub replicas: Vec<Arc<StreamReplica>>,
    /// This stream's control group record.
    pub control_groups: Vec<Arc<ControlGroup>>,
}

/// An event indicating that a new pipeline was created.
pub struct PipelineCreated {
    /// The pipeline's data model.
    pub pipeline: Arc<WithId<Pipeline>>,
    /// The replicas associated with this pipeline.
    pub replicas: Vec<Arc<PipelineReplica>>,
    /// This pipeline's control group record.
    pub control_group: Arc<ControlGroup>,
}

/// An event indicating that a stream replica's node assignment was updated.
pub struct StreamReplicaAssignmentUpdated {
    /// The stream replica's data model.
    pub replica: Arc<StreamReplica>,
    /// The type of change which was applied.
    pub change: Assignment,
}
