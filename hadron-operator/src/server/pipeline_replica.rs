#![allow(dead_code)] // TODO: remove.

use std::sync::Arc;

use crate::crd::{Pipeline, Stream};
use crate::server::H2Channel;

/// A message bound for a pipeline replication controller.
pub enum PipelineReplicaCtlMsg {
    /// A cluster request being routed to the controller.
    Request(H2Channel),
    /// An update to the controller's pipeline object.
    PipelineUpdated(Arc<Pipeline>, Arc<Stream>),
    /// An update indicating that this pipeline has been deleted.
    PipelineDeleted(Arc<Pipeline>),
}
