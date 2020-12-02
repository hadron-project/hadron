mod app;
mod auth;
mod config;
mod core;
mod discovery;
mod models;
mod network;
mod proto;
mod storage;
mod utils;

// Public exports for binaries.
pub use crate::{app::App, config::Config};

/// A Railgun cluster node's ID.
type NodeId = u64;

/* TODO:
- ensure ephemeral & RPC messaging is fast. All publishing & subscriptions should converge on the
the master even via direct connection or forwarding.
*/
