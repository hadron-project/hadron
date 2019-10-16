use actix::prelude::*;
use actix_raft::RaftMetrics;

use crate::{
    NodeId,
    networking::Network,
};

//////////////////////////////////////////////////////////////////////////////////////////////////
// RaftMetrics ///////////////////////////////////////////////////////////////////////////////////

impl Handler<RaftMetrics> for Network {
    type Result = ();

    fn handle(&mut self, msg: RaftMetrics, ctx: &mut Context<Self>) -> Self::Result {
        // Update leader tracking (will forward buffered client requests).
        self.update_current_leader(msg.current_leader, ctx);

        // TODO: finish this up with prometheus and general metrics integrations.
        log::debug!("{:?}", msg)
    }
}

impl Network {
    /// Update the tracked value of the Raft cluster's current leader.
    fn update_current_leader(&mut self, val: Option<NodeId>, ctx: &mut Context<Self>) {
        self.current_leader = val;
        if self.current_leader.is_some() && self.forwarding_buffer.len() > 0 {
            self.forward_buffered_client_requests(ctx);
        }

        // Update delivery actor with leader update.
        let _ = self.leader_update_stream.unbounded_send(val);
    }
}
