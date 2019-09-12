use actix::prelude::*;
use actix_raft::{
    RaftNetwork,
    messages::{
        AppendEntriesRequest, AppendEntriesResponse,
        InstallSnapshotRequest, InstallSnapshotResponse,
        VoteRequest, VoteResponse,
    },
};

use crate::{
    db::AppData,
    networking::Network,
};

//////////////////////////////////////////////////////////////////////////////////////////////////
// RaftNetwork impl //////////////////////////////////////////////////////////////////////////////

type RgAppendEntriesRequest = AppendEntriesRequest<AppData>;

impl RaftNetwork<AppData> for Network {}

impl Handler<RgAppendEntriesRequest> for Network {
    type Result = ResponseActFuture<Self, AppendEntriesResponse, ()>;

    fn handle(&mut self, _msg: RgAppendEntriesRequest, _ctx: &mut Self::Context) -> Self::Result {
        Box::new(fut::err(()))
    }
}

impl Handler<InstallSnapshotRequest> for Network {
    type Result = ResponseActFuture<Self, InstallSnapshotResponse, ()>;

    fn handle(&mut self, _msg: InstallSnapshotRequest, _ctx: &mut Self::Context) -> Self::Result {
        Box::new(fut::err(()))
    }
}

impl Handler<VoteRequest> for Network {
    type Result = ResponseActFuture<Self, VoteResponse, ()>;

    fn handle(&mut self, _msg: VoteRequest, _ctx: &mut Self::Context) -> Self::Result {
        Box::new(fut::err(()))
    }
}
