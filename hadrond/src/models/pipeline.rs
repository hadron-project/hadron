use std::collections::HashMap;
use std::sync::Arc;

use bytes::Bytes;
use uuid::Uuid;

pub use crate::models::proto::pipeline::*;

/// An active pipeline instance along with all outputs from any completed stages.
#[derive(Debug)]
pub struct ActivePipelineInstance {
    /// The model of the pipeline instance, directly from storage.
    pub instance: PipelineInstance,
    /// A copy of the stream record which triggered this instance.
    pub root_event: Bytes,
    /// A mapping of stage names to their completion outputs.
    pub outputs: HashMap<String, PipelineStageOutput>,
    /// A mapping of active deliveries by stage name to the channel ID currently processing
    /// the stage's delivery.
    pub active_deliveries: HashMap<Arc<String>, Uuid>,
}
