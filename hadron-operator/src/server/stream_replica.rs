#![allow(dead_code)] // TODO: remove.

use std::sync::Arc;

use crate::crd::Stream;
use crate::server::H2Channel;

/// A message bound for a stream replication controller.
pub enum StreamReplicaCtlMsg {
    /// A cluster request being routed to the controller.
    Request(H2Channel),
    /// An update to the controller's stream object.
    StreamUpdated(Arc<Stream>),
    /// An update indicating that this stream has been deleted.
    StreamDeleted(Arc<Stream>),
}
