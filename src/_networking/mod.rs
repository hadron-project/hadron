// mod client;
// mod from_peer;
// mod network;
// mod to_peer;

use std::collections::HashMap;

pub use network::{Network, NetworkServices};

use crate::proto::peer::ClientInfo;

/// A mapping of client connection IDs to their associated client info object.
pub(crate) type ClientRoutingInfo = HashMap<String, ClientInfo>;
