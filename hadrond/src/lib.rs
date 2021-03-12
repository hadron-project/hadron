mod app;
mod auth;
mod config;
mod ctl_placement;
mod ctl_raft;
mod ctl_stream;
mod database;
mod discovery;
mod error;
mod models;
mod network;
mod proto;
mod raft;
mod utils;

// Public exports for binaries.
pub use crate::{app::App, config::Config};

/// A Hadron cluster node's ID.
type NodeId = u64;
