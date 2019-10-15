use std::time::Duration;

use actix::prelude::*;
use actix_raft::{
    RaftNetwork,
    messages::{
        AppendEntriesRequest, AppendEntriesResponse,
        InstallSnapshotRequest, InstallSnapshotResponse,
        VoteRequest, VoteResponse,
    },
};
use futures::future::err as fut_err;
use log::{error};

use crate::{
    app::AppData,
    networking::network::{Network, OutboundPeerRequest},
    proto::peer,
};

/// The default timeout for Raft AppendEntries RPCs.
///
/// TODO: we should update this to come from runtime config for user control.
pub(self) const RAFT_APPEND_ENTRIES_TIMEOUT: Duration = Duration::from_secs(5);
/// The default timeout for Raft InstallSnapshot RPCs.
///
/// TODO: we should update this to come from runtime config for user control.
pub(self) const RAFT_INSTALL_SNAPSHOT_TIMEOUT: Duration = Duration::from_secs(5);
/// The default timeout for Raft VoteRequest RPCs.
///
/// TODO: we should update this to be half of the election timeout min of runtime config.
pub(self) const RAFT_VOTE_REQUEST_TIMEOUT: Duration = Duration::from_secs(1);

//////////////////////////////////////////////////////////////////////////////////////////////////
// RaftNetwork impl //////////////////////////////////////////////////////////////////////////////

type RgAppendEntriesRequest = AppendEntriesRequest<AppData>;

impl RaftNetwork<AppData> for Network {}

impl Handler<RgAppendEntriesRequest> for Network {
    type Result = ResponseFuture<AppendEntriesResponse, ()>;

    fn handle(&mut self, msg: RgAppendEntriesRequest, ctx: &mut Self::Context) -> Self::Result {
        let payload = match bincode::serialize(&msg) {
            Err(err) => {
                error!("Error serializing outbound AppendEntriesRequest. {}", err);
                return Box::new(fut_err(()));
            }
            Ok(payload) => payload,
        };
        let request = peer::Request{segment: Some(peer::request::Segment::Raft(peer::RaftRequest{
            payload: Some(peer::raft_request::Payload::AppendEntries(payload)),
        }))};
        let outbound = OutboundPeerRequest{request, target_node: msg.target, timeout: RAFT_APPEND_ENTRIES_TIMEOUT};

        Box::new(self.send_outbound_peer_request(outbound, ctx)
            .and_then(|frame| {
                let raft_frame = match frame.segment {
                    Some(peer::response::Segment::Raft(raft_frame)) => raft_frame,
                    _ => return Err(()),
                };
                let data = match raft_frame.payload {
                    Some(peer::raft_response::Payload::AppendEntries(data)) => data,
                    _ => return Err(()),
                };
                let res: AppendEntriesResponse = bincode::deserialize(&data).map_err(|err| {
                    error!("Error deserializing AppendEntriesResponse. {}", err);
                    ()
                })?;
                Ok(res)
            }))
    }
}

impl Handler<InstallSnapshotRequest> for Network {
    type Result = ResponseFuture<InstallSnapshotResponse, ()>;

    fn handle(&mut self, msg: InstallSnapshotRequest, ctx: &mut Self::Context) -> Self::Result {
        let payload = match bincode::serialize(&msg) {
            Err(err) => {
                error!("Error serializing outbound InstallSnapshotRequest. {}", err);
                return Box::new(fut_err(()));
            }
            Ok(payload) => payload,
        };
        let request = peer::Request{segment: Some(peer::request::Segment::Raft(peer::RaftRequest{
            payload: Some(peer::raft_request::Payload::InstallSnapshot(payload)),
        }))};
        let outbound = OutboundPeerRequest{request, target_node: msg.target, timeout: RAFT_INSTALL_SNAPSHOT_TIMEOUT};

        Box::new(self.send_outbound_peer_request(outbound, ctx)
            .and_then(|frame| {
                let raft_frame = match frame.segment {
                    Some(peer::response::Segment::Raft(raft_frame)) => raft_frame,
                    _ => return Err(()),
                };
                let data = match raft_frame.payload {
                    Some(peer::raft_response::Payload::InstallSnapshot(data)) => data,
                    _ => return Err(()),
                };
                let res: InstallSnapshotResponse = bincode::deserialize(&data).map_err(|err| {
                    error!("Error deserializing InstallSnapshotResponse. {}", err);
                    ()
                })?;
                Ok(res)
            }))
    }
}

impl Handler<VoteRequest> for Network {
    type Result = ResponseFuture<VoteResponse, ()>;

    fn handle(&mut self, msg: VoteRequest, ctx: &mut Self::Context) -> Self::Result {
        let payload = match bincode::serialize(&msg) {
            Err(err) => {
                error!("Error serializing outbound VoteRequest. {}", err);
                return Box::new(fut_err(()));
            }
            Ok(payload) => payload,
        };
        let request = peer::Request{segment: Some(peer::request::Segment::Raft(peer::RaftRequest{
            payload: Some(peer::raft_request::Payload::Vote(payload)),
        }))};
        let outbound = OutboundPeerRequest{request, target_node: msg.target, timeout: RAFT_VOTE_REQUEST_TIMEOUT};

        Box::new(self.send_outbound_peer_request(outbound, ctx)
            .and_then(|frame| {
                let raft_frame = match frame.segment {
                    Some(peer::response::Segment::Raft(raft_frame)) => raft_frame,
                    _ => return Err(()),
                };
                let data = match raft_frame.payload {
                    Some(peer::raft_response::Payload::Vote(data)) => data,
                    _ => return Err(()),
                };
                let res: VoteResponse = bincode::deserialize(&data).map_err(|err| {
                    error!("Error deserializing VoteResponse. {}", err);
                    ()
                })?;
                Ok(res)
            }))
    }
}
