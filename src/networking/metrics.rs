use actix::prelude::*;
use actix_raft::RaftMetrics;

use crate::networking::Network;

impl Handler<RaftMetrics> for Network {
    type Result = ();

    fn handle(&mut self, _msg: RaftMetrics, _ctx: &mut Context<Self>) -> Self::Result {
        // TODO: finish implementing this.
    }
}
