use std::{fmt, error};

use crate::proto::peer;

impl peer::Frame {
    /// Create a new disconnect frame.
    pub fn new_disconnect(disconnect: peer::Disconnect, meta: peer::Meta) -> Self {
        Self{
            meta: Some(meta),
            payload: Some(peer::frame::Payload::Disconnect(disconnect as i32)),
        }
    }
}

impl peer::Request {
    /// Create a new forwarding request.
    pub fn new_forwarded(payload: Vec<u8>) -> Self {
        Self{segment: Some(peer::request::Segment::Forwarded(peer::ForwardedClientRequest{payload}))}
    }
}

impl peer::Response {
    /// Create a new error instance.
    pub fn new_error(err: peer::Error) -> Self {
        Self{segment: Some(peer::response::Segment::Error(err as i32))}
    }
}

impl fmt::Display for peer::Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl error::Error for peer::Error {}
