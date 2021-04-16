//! CRC application commands.
//!
//! The CRC is responsible for controlling the state of the Hadron cluster as a whole, and exposes
//! a control signal to the application in order to drive the system. That logic is encapsulated here.

use std::sync::Arc;

use crate::models::schema::{Pipeline, Stream};

/// An event describing some state change in the system.
pub enum Event {
    /// An initial payload of data from the CRC.
    Initial(InitialState),
    /// An event indicating that a new stream was created.
    StreamCreated(StreamCreated),
    /// An event indicating that a new pipeline was created.
    PipelineCreated(PipelineCreated),
}

/// An initial payload of data following system state recovery.
pub struct InitialState {
    /// All known streams in the cluster.
    pub streams: Vec<Arc<Stream>>,
    /// All known pipelines in the cluster.
    pub pipelines: Vec<Arc<Pipeline>>,
}

impl InitialState {
    /// Create a new instance.
    pub fn new(streams: Vec<Arc<Stream>>, pipelines: Vec<Arc<Pipeline>>) -> Self {
        Self { streams, pipelines }
    }
}

/// An event indicating that a new stream was created.
pub struct StreamCreated {
    /// The stream's data model.
    pub stream: Arc<Stream>,
}

/// An event indicating that a new pipeline was created.
pub struct PipelineCreated {
    /// The pipeline's data model.
    pub pipeline: Arc<Pipeline>,
}
