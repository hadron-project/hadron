//! The consensus module.
//!
//! This module encapsulates the `Consensus` actor and all other actors and types directly related
//! to the consensus system. Cluster consensus is implemented using the Raft protocol.

use std::{
    sync::Arc,
    time::Duration,
};

use actix::prelude::*;
use log::{info};
use raft::{
    storage::MemStorage,
    raw_node::RawNode,
};

use crate::{
    App,
    common::NodeId,
    config::Config,
    db::{RaftStorage, StreamStorage},
};

/// An actor type encapsulating the consensus system based on the Raft protocol.
pub struct Consensus {
    app: Addr<App>,
    node_id: NodeId,
    storage: RaftStorage,
    cluster_raft: RawNode<MemStorage>,

    streams_arb: Arbiter,
    streams_storage: Addr<StreamStorage>,

    _config: Arc<Config>,
}

impl Consensus {
    /// Create a new instance.
    ///
    /// This will load the last configured state of the cluster Raft from disk if it is present.
    /// If not, then the default node configuration will be used. If there are any known streams
    /// recorded in the cluster state record, or new streams are added during runtime, then a new
    /// WriterDelegate actor will be spawned onto a dedicated arbiter.
    ///
    /// TODO:NOTE: data flowing in from a client to the networking layer will have to propagate
    /// the frame to the app, then to this actor, then to the writer delegate. This path could be
    /// optimized by caching some routing info in the networking layer so that the connections
    /// actor might be able to route inbound frames more quickly.
    pub fn new(
        app: Addr<App>, node_id: NodeId, storage: RaftStorage,
        streams_arb: Arbiter, streams_storage: Addr<StreamStorage>, config: Arc<Config>,
    ) -> Result<Self, String> {
        // Build the cluster Raft.
        let cluster_config = raft::Config{id: node_id, ..Default::default()};
        let cluster_raft = RawNode::new(&cluster_config, MemStorage::new(), Vec::with_capacity(0)).map_err(|err| err.to_string())?;

        Ok(Consensus{app, node_id, storage, cluster_raft, streams_arb, streams_storage, _config: config})
    }

    // Drive the Raft.
    fn drive_raft(&mut self, _ctx: &mut Context<Self>) {
        // TODO: impl according to Raft docs.
    }
}

impl Actor for Consensus {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        info!("Starting the consensus system.");

        // Setup an interval operation to drive the cluster Raft.
        ctx.run_interval(Duration::from_millis(150), |a, c| a.drive_raft(c));
    }
}
