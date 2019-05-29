//! The consensus module.
//!
//! This module encapsulates the `Consensus` actor and all other actors and types directly related
//! to the consensus system. Cluster consensus is implemented using the Raft protocol.

use actix::prelude::*;
use log::{info};

/// An actor type encapsulating the consensus system based on the Raft protocol.
pub struct Consensus;

impl Actor for Consensus {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        info!("Starting the cluster consensus system.");
    }
}
