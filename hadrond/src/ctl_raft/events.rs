//! CRC application commands.
//!
//! The CRC is responsible for controlling the state of the Hadron cluster as a whole, and exposes
//! a control signal to the application in order to drive the system. That logic is encapsulated here.

use crate::ctl_placement::models::{PipelineReplica, StreamReplica};
use crate::models::{Pipeline, Stream};

/// An event coming from the CRC.
pub enum CRCEvent {
    /// An initial payload of data from the CRC.
    Initial(InitialEvent),
    /// An event indicating that a new stream was created.
    StreamCreated(StreamCreated),
    /// An event indicating that a new stream replica was created.
    StreamReplicaCreated(StreamReplicaCreated),
    /// An event indicating that a new pipeline was created.
    PipelineCreated(PipelineCreated),
    /// An event indicating that a new pipeline replica was created.
    PipelineReplicaCreated(PipelineReplicaCreated),
}

/// An initial payload of data from the CRC.
///
/// This event will be emitted from the CRC when it is first initialized after recovering its
/// state from storage.
pub struct InitialEvent {
    /// All known streams in the cluster.
    pub streams: Vec<Stream>,
    /// All known stream replicas in the cluster.
    pub stream_replicas: Vec<StreamReplica>,
    /// All known pipelines in the cluster.
    pub pipelines: Vec<Pipeline>,
    /// All known pipeline replicas in the cluster.
    pub pipeline_replicas: Vec<PipelineReplica>,
}

impl InitialEvent {
    /// Create a new instance.
    pub fn new(streams: Vec<Stream>, stream_replicas: Vec<StreamReplica>, pipelines: Vec<Pipeline>, pipeline_replicas: Vec<PipelineReplica>) -> Self {
        Self {
            streams,
            stream_replicas,
            pipelines,
            pipeline_replicas,
        }
    }
}

/// An event indicating that a new stream was created.
pub struct StreamCreated {
    /// The stream's data model.
    pub stream: Stream,
}

/// An event indicating that a new stream replica was created.
pub struct StreamReplicaCreated {
    /// The stream replica's data model.
    pub replica: StreamReplica,
}

/// An event indicating that a new pipeline was created.
pub struct PipelineCreated {
    /// The pipeline's data model.
    pub pipeline: Pipeline,
}

/// An event indicating that a new pipeline replica was created.
pub struct PipelineReplicaCreated {
    /// The pipeline replica's data model.
    pub replica: PipelineReplica,
}
