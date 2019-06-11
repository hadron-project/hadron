// //! The consensus module.
// //!
// //! This module encapsulates the `Consensus` actor and all other actors and types directly related
// //! to the consensus system. Cluster consensus is implemented using the Raft protocol.

// use std::{
//     sync::Arc,
//     time::Duration,
// };

// use actix::prelude::*;
// use log::{info};
// use raft::{
//     storage::MemStorage,
//     raw_node::RawNode,
// };

// use crate::{
//     App,
//     common::NodeId,
//     config::Config,
//     db::{RaftStorage, StreamStorage},
//     proto::storage::raft::ClusterState,
// };

// /// An actor type encapsulating the consensus system based on the Raft protocol.
// pub struct Consensus {
//     app: Addr<App>,
//     node_id: NodeId,
//     storage: RaftStorage,

//     is_in_drive: bool,
//     cluster_raft: RawNode<MemStorage>,

//     streams_arb: Arbiter,
//     streams_storage: Addr<StreamStorage>,

//     _config: Arc<Config>,
// }

// impl Consensus {
//     /// Create a new instance.
//     ///
//     /// This will load the last configured state of the cluster Raft from disk if it is present.
//     /// If not, then the default node configuration will be used. If there are any known streams
//     /// recorded in the cluster state record, or new streams are added during runtime, then a new
//     /// WriterDelegate actor will be spawned onto a dedicated arbiter.
//     ///
//     /// TODO:NOTE: data flowing in from a client to the networking layer will have to propagate
//     /// the frame to the app, then to this actor, then to the writer delegate. This path could be
//     /// optimized by caching some routing info in the networking layer so that the `Network`
//     /// actor might be able to route inbound frames more quickly.
//     pub fn new(
//         app: Addr<App>, node_id: NodeId, storage: RaftStorage, cluster_state: ClusterState,
//         streams_arb: Arbiter, streams_storage: Addr<StreamStorage>, config: Arc<Config>,
//     ) -> Result<Self, String> {
//         // Build the cluster Raft.
//         let cluster_config = raft::Config{
//             id: node_id,
//             applied: cluster_state.raft.commit,
//             ..Default::default()
//         };
//         let cluster_raft = RawNode::new(&cluster_config, MemStorage::new(), Vec::with_capacity(0))
//             .map_err(|err| err.to_string())?;

//         Ok(Consensus{
//             app, node_id, storage,
//             is_in_drive: false, cluster_raft,
//             streams_arb, streams_storage,
//             _config: config
//         })
//     }

//     // Drive the Raft.
//     fn drive_raft(&mut self, ctx: &mut Context<Self>) {
//         self.cluster_raft.tick();
//         let is_leader = self.cluster_raft.status().ss.leader_id == self.node_id;

//         // If the node has ready state, continue.
//         if !self.cluster_raft.has_ready() {
//             return;
//         }
//         let mut ready = self.cluster_raft.ready();

//         // Process the different stages for ticking the Raft.
//         self.raft_tick_stage_1(ctx, is_leader, &mut ready);
//         self.raft_tick_stage_2(ctx, is_leader, &mut ready);
//         self.raft_tick_stage_3(ctx, is_leader, &mut ready);
//         self.raft_tick_stage_4(ctx, is_leader, &mut ready);
//         self.raft_tick_stage_5(ctx, is_leader, &mut ready);
//         self.cluster_raft.advance(ready);
//     }

//     /// Handle stage 1 of a Raft tick.
//     fn raft_tick_stage_1(&mut self, _ctx: &mut Context<Self>, is_leader: bool, ready: &mut raft::Ready) {
//         // If this node is not the leader, then we must respond to the leader for each message.
//         if is_leader {
//             for _msg in ready.messages.drain(..) {
//                 // TODO: Send messages to peers.
//                 // TODO: handle snapshot messages.
//             }
//         }

//         // TODO: impl this. Will depend on RaftStorage impl.
//         // // Check whether snapshot is empty or not. If not empty, it means that the Raft node
//         // // has received a Raft snapshot from the leader and we must apply the snapshot.
//         else if !raft::is_empty_snap(ready.snapshot()) {
//             // if !raft::is_empty_snap(ready.snapshot()) {
//             //     // This is a snapshot, we need to apply the snapshot at first.
//             //     node.mut_store()
//             //         .wl()
//             //         .apply_snapshot(ready.snapshot().clone())
//             //         .unwrap();
//             // }
//         }
//     }

//     /// Handle stage 2 of a Raft tick.
//     fn raft_tick_stage_2(&mut self, _ctx: &mut Context<Self>, _is_leader: bool, ready: &mut raft::Ready) {
//         // Check whether entries is empty or not. If not empty, it means that there are newly
//         // added entries which have not been committed yet which must be appended to the Raft log.
//         if !ready.entries().is_empty() {
//             // TODO: Append entries to the Raft log.
//             // self.cluster_raft.mut_store().wl().append(ready.entries()).unwrap();
//         }
//     }

//     /// Handle stage 3 of a Raft tick.
//     fn raft_tick_stage_3(&mut self, _ctx: &mut Context<Self>, _is_leader: bool, ready: &mut raft::Ready) {
//         // Check whether `hs` is empty or not. If not empty, it means that the `HardState` of the
//         // node has changed. For example, the node may vote for a new leader, or the commit index
//         // has been increased. We must persist the changed `HardState`.
//         if let Some(_hs) = ready.hs() {
//             // TODO: Raft HardState changed, and we need to persist it.
//             // node.mut_store().wl().set_hardstate(hs.clone());
//         }
//     }

//     /// Handle stage 4 of a Raft tick.
//     fn raft_tick_stage_4(&mut self, _ctx: &mut Context<Self>, is_leader: bool, ready: &mut raft::Ready) {
//         // If this node is not the leader, then we must respond to the leader for each message.
//         if !is_leader {
//             for _msg in ready.messages.drain(..) {
//                 // TODO: Send messages to leader.
//                 // TODO: handle snapshot messages.
//             }
//         }
//     }

//     /// Handle stage 5 of a Raft tick.
//     fn raft_tick_stage_5(&mut self, _ctx: &mut Context<Self>, _is_leader: bool, ready: &mut raft::Ready) {
//         // Check for newly committed log entries which must be applied to the state machine.
//         if let Some(committed_entries) = ready.committed_entries.take() {
//             let mut _last_applied_index = self.cluster_raft.status().applied;
//             for entry in committed_entries {
//                 // Raft leader will send empty entries as heartbeat according to Raft spec.
//                 _last_applied_index = entry.get_index();
//                 if entry.get_data().is_empty() {
//                     continue;
//                 }

//                 // Handle applying entries to the database.
//                 use raft::eraftpb::EntryType::{EntryNormal, EntryConfChange};
//                 match entry.get_entry_type() {
//                     EntryNormal => (), // TODO: handle normal entries.
//                     EntryConfChange => (), // TODO: handle config changes.
//                 }
//             }

//             // Persist last applied index for easy resume after a restart.
//             // TODO: ^^^
//         }
//     }
// }

// impl Actor for Consensus {
//     type Context = Context<Self>;

//     fn started(&mut self, ctx: &mut Self::Context) {
//         info!("Starting the consensus system.");

//         // Setup an interval operation to drive the cluster Raft.
//         ctx.run_interval(Duration::from_millis(50), |a, c| {
//             if a.is_in_drive {
//                 return;
//             }
//             a.is_in_drive = true;
//             a.drive_raft(c);
//             a.is_in_drive = false;
//         });
//     }
// }

// // TODO: handle standard node message entries to be appended using self.cluster_raft.propose()
// // TODO: handle config change messages to be appended using self.cluster_raft.propose_conf_change()
