#![allow(dead_code)] // TODO: remove this

use std::time::Duration;

/// The default interval for the HTTP2 keep alive interval.
pub(crate) const DEFAULT_H2_KEEP_ALIVE_INTERVAL: Duration = Duration::from_secs(15);
/// The default timeout for the HTTP2 keep alive.
pub(crate) const DEFAULT_H2_KEEP_ALIVE_TIMEOUT: Duration = Duration::from_secs(7);
