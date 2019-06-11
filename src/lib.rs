#[macro_use]
extern crate serde;

mod app;
mod config;
mod networking;
mod consensus;
mod db;
mod discovery;
mod proto;

// use serde::{Serialize, Deserialize};

// Public exports for binaries.
pub use crate::{
    app::App,
    config::Config,
};

/// A Railgun cluster node's ID.
type NodeId = u64;
