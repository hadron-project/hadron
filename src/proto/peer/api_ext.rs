use crate::proto::peer::{
    api::{Disconnect, Frame, frame, Meta},
};

impl Frame {
    /// Create a new disconnect frame.
    pub fn new_disconnect(disconnect: Disconnect, meta: Meta) -> Self {
        Frame{
            meta: Some(meta),
            payload: Some(frame::Payload::Disconnect(disconnect as i32)),
        }
    }
}
