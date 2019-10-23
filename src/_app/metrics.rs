use actix::prelude::*;
use actix_raft::RaftMetrics;

use crate::{
    NodeId,
    app::App,
};

//////////////////////////////////////////////////////////////////////////////////////////////////
// RaftMetrics ///////////////////////////////////////////////////////////////////////////////////

impl Handler<RaftMetrics> for App {
    type Result = ();

    fn handle(&mut self, msg: RaftMetrics, ctx: &mut Context<Self>) -> Self::Result {
        // Update current leader & emit leader update only if the value has changed.
        if &self.current_leader != &msg.current_leader {
            self.update_current_leader(msg.current_leader, ctx);
        }

        // Publish an update on the metrics channel.
        self.metrics.publish(msg);
    }
}

impl App {
    /// Update the tracked value of the Raft cluster's current leader & publish an update on the leader channel.
    fn update_current_leader(&mut self, val: Option<NodeId>, ctx: &mut Context<Self>) {
        self.current_leader = val;
        self.leader.publish(val);
    }
}
