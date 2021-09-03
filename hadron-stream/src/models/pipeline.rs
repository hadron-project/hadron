use std::collections::HashMap;
use std::sync::Arc;

use uuid::Uuid;

use crate::grpc::Event;

/// An active pipeline instance along with all outputs from any completed stages.
#[derive(Debug)]
pub struct ActivePipelineInstance {
    /// A copy of the stream event which triggered this instance.
    pub root_event: Event,
    /// A mapping of stage names to their completion outputs.
    pub outputs: HashMap<String, Event>,
    /// A mapping of active deliveries by stage name to the channel ID currently processing
    /// the stage's delivery.
    pub active_deliveries: HashMap<Arc<String>, Uuid>,
}
