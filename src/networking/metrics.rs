use actix::prelude::*;
use actix_raft::RaftMetrics;
use log::{info};

use crate::networking::Network;

impl Handler<RaftMetrics> for Network {
    type Result = ();

    fn handle(&mut self, msg: RaftMetrics, _ctx: &mut Context<Self>) -> Self::Result {
        // TODO: finish this up with prometheus and general metrics integrations.
        info!("{:?}", msg)
    }
}
