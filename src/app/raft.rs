use actix::prelude::*;
use actix_raft::{
    RaftMetrics,
    messages::{
        AppendEntriesRequest,
        ClientError as ClientPayloadError,
        VoteRequest,
        InstallSnapshotRequest,
    },
};
use log;

use crate::{
    app::{
        App, AppData, AppDataError,
        RgClientPayload, RgClientPayloadResponse, RgClientPayloadError
    },
    proto::peer,
};

impl App {
    fn handle_raft_append_entries_request(&mut self, req: AppendEntriesRequest<AppData>, _: &mut Context<Self>) -> impl ActorFuture<Actor=Self, Item=peer::api::RaftResponse, Error=peer::api::Error> {
        fut::wrap_future(self.raft.send(req)
            .map_err(|err| {
                log::error!("Error handling Raft AppendEntriesRequest. {}", err);
                peer::api::Error::Internal
            }))
            .and_then(|res, _, _| fut::result(res).map_err(|_, _, _| peer::api::Error::Internal))
            .and_then(|res, _, _| fut::result(bincode::serialize(&res).map_err(|err| {
                log::error!("Error serializing AppendEntriesResponse. {}", err);
                peer::api::Error::Internal
            })))
            .map(|data, _, _| peer::api::RaftResponse{payload: Some(peer::api::raft_response::Payload::AppendEntries(data))})
    }

    fn handle_raft_vote_request(&mut self, req: VoteRequest, _: &mut Context<Self>) -> impl ActorFuture<Actor=Self, Item=peer::api::RaftResponse, Error=peer::api::Error> {
        fut::wrap_future(self.raft.send(req)
            .map_err(|err| {
                log::error!("Error handling Raft VoteRequest. {}", err);
                peer::api::Error::Internal
            }))
            .and_then(|res, _, _| fut::result(res).map_err(|_, _, _| peer::api::Error::Internal))
            .and_then(|res, _, _| fut::result(bincode::serialize(&res).map_err(|err| {
                log::error!("Error serializing VoteResponse. {}", err);
                peer::api::Error::Internal
            })))
            .map(|data, _, _| peer::api::RaftResponse{payload: Some(peer::api::raft_response::Payload::Vote(data))})
    }

    fn handle_raft_install_snapshot_request(&mut self, req: InstallSnapshotRequest, _: &mut Context<Self>) -> impl ActorFuture<Actor=Self, Item=peer::api::RaftResponse, Error=peer::api::Error> {
        fut::wrap_future(self.raft.send(req)
            .map_err(|err| {
                log::error!("Error handling Raft InstallSnapshotRequest. {}", err);
                peer::api::Error::Internal
            }))
            .and_then(|res, _, _| fut::result(res).map_err(|_, _, _| peer::api::Error::Internal))
            .and_then(|res, _, _| fut::result(bincode::serialize(&res).map_err(|err| {
                log::error!("Error serializing InstallSnapshotResponse. {}", err);
                peer::api::Error::Internal
            })))
            .map(|data, _, _| peer::api::RaftResponse{payload: Some(peer::api::raft_response::Payload::InstallSnapshot(data))})
    }
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// ClientPayload /////////////////////////////////////////////////////////////////////////////////

impl Handler<RgClientPayload> for App {
    type Result = ResponseFuture<RgClientPayloadResponse, RgClientPayloadError>;

    fn handle(&mut self, msg: RgClientPayload, _ctx: &mut Context<Self>) -> Self::Result {
        Box::new(self.raft.send(msg)
            .map_err(|_| ClientPayloadError::Application(AppDataError::Internal))
            .and_then(|res| res))
    }
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// InboundRaftRequest ////////////////////////////////////////////////////////////////////////////

/// A message type wrapping an inbound peer API request along with its metadata.
pub struct InboundRaftRequest(pub peer::api::RaftRequest, pub peer::api::Meta);

impl Message for InboundRaftRequest {
    type Result = Result<peer::api::RaftResponse, peer::api::Error>;
}

impl Handler<InboundRaftRequest> for App {
    type Result = ResponseActFuture<Self, peer::api::RaftResponse, peer::api::Error>;

    /// Handle inbound peer API requests.
    fn handle(&mut self, msg: InboundRaftRequest, _ctx: &mut Self::Context) -> Self::Result {
        let (req, _meta) = (msg.0, msg.1);
        use peer::api::raft_request::Payload;
        match req.payload {
            Some(Payload::AppendEntries(data)) => Box::new(fut::result(bincode::deserialize::<AppendEntriesRequest<AppData>>(data.as_slice())
                .map_err(|err| {
                    log::error!("Error deserializing inbound AppendEntriesRequest. {}", err);
                    peer::api::Error::Internal
                }))
                .and_then(|req, act: &mut Self, ctx| act.handle_raft_append_entries_request(req, ctx))),
            Some(Payload::Vote(data)) => Box::new(fut::result(bincode::deserialize::<VoteRequest>(data.as_slice())
                .map_err(|err| {
                    log::error!("Error deserializing inbound VoteRequest. {}", err);
                    peer::api::Error::Internal
                }))
                .and_then(|req, act: &mut Self, ctx| act.handle_raft_vote_request(req, ctx))),
            Some(Payload::InstallSnapshot(data)) => Box::new(fut::result(bincode::deserialize::<InstallSnapshotRequest>(data.as_slice())
                .map_err(|err| {
                    log::error!("Error deserializing inbound InstallSnapshotRequest. {}", err);
                    peer::api::Error::Internal
                }))
                .and_then(|req, act: &mut Self, ctx| act.handle_raft_install_snapshot_request(req, ctx))),
            _ => {
                log::error!("Unknown Raft request variant received.");
                Box::new(fut::err(peer::api::Error::Internal))
            }
        }
    }
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// RaftMetrics ///////////////////////////////////////////////////////////////////////////////////

impl Handler<RaftMetrics> for App {
    type Result = ();

    fn handle(&mut self, msg: RaftMetrics, _ctx: &mut Context<Self>) -> Self::Result {
        // TODO: finish this up with prometheus and general metrics integrations.
        log::debug!("{:?}", msg)
    }
}
