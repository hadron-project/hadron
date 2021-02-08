mod app;
mod auth;
mod config;
mod crc;
mod discovery;
mod error;
mod models;
mod network;
mod proto;
mod storage;
mod utils;

// Public exports for binaries.
pub use crate::{app::App, config::Config};

/// A Hadron cluster node's ID.
type NodeId = u64;
