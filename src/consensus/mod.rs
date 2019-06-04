//! The consensus module.
//!
//! This module encapsulates the `Consensus` actor and all other actors and types directly related
//! to the consensus system. Cluster consensus is implemented using the Raft protocol.

use std::sync::Arc;

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
    db::RaftStorage,
};

/// An actor type encapsulating the consensus system based on the Raft protocol.
pub struct Consensus {
    app: Addr<App>,
    node_id: NodeId,
    storage: RaftStorage,
    cluster_raft: RawNode<MemStorage>,

    _config: Arc<Config>,
}

impl Consensus {
    /// Create a new instance.
    ///
    /// This system implements the multi-raft protocol, acting as a Raft Group. The cluster as a
    /// whole is the primary Raft which is responsible for cross-cluster data. Each persistent
    /// stream gets its own Raft as part of the group as well.
    ///
    /// This will load the last configured state of the cluster Raft from disk if it is present.
    /// If not, then the default node configuration will be used. If there are any known streams
    /// recorded in the cluster state record, then a new Raft instance will be created per stream.
    pub fn new(app: Addr<App>, node_id: NodeId, storage: RaftStorage, config: Arc<Config>) -> Result<Self, String> {
        // Extract the cluster state from disk, or default if not present.
        let _cluster_state = storage.get_cluster_state().map_err(|err| err.to_string())?;

        // Build the cluster Raft.
        let cluster_config = raft::Config{id: node_id, ..Default::default()};
        let cluster_raft = RawNode::new(&cluster_config, MemStorage::new(), Vec::with_capacity(0)).map_err(|err| err.to_string())?;

        // Build any additional Rafts for any streams currently recorded in the system.
        // TODO: finish this up.

        Ok(Consensus{app, node_id, storage, cluster_raft, _config: config})
    }
}

impl Actor for Consensus {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        info!("Starting the cluster consensus system.");

        // TODO:
        // - setup raft tick for processing the entire raft group.
        //   - outbound heartbeats will be batched per target node.
        //   -

        // NEED TO CONSIDER:
        // - batching of outbound heartbeats is fine, but what about other network activity? As
        //   data is written and replicated, this counts as heartbeat, but not every Raft in the
        //   group will get a heartbeat.
    }
}
