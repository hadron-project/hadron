use std::collections::HashMap;
use std::sync::Arc;

use sled::IVec;
use uuid::Uuid;

use crate::grpc::Event;
pub use crate::models::proto::pipeline::*;

/// An active pipeline instance along with all outputs from any completed stages.
#[derive(Debug)]
pub struct ActivePipelineInstance {
    /// A copy of the stream event which triggered this instance.
    pub root_event: Event,
    /// A copy of the raw bytes of the root event.
    pub root_event_bytes: IVec,
    /// A mapping of stage names to their completion outputs.
    pub outputs: HashMap<String, PipelineStageOutput>,
    /// A mapping of active deliveries by stage name to the channel ID currently processing
    /// the stage's delivery.
    pub active_deliveries: HashMap<Arc<String>, Uuid>,
}
