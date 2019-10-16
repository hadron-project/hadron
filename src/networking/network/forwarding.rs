use std::time::Duration;

use actix::{
    prelude::*,
    fut::FutureResult,
};
use futures::sync::oneshot;
use uuid::Uuid;

use crate::{
    NodeId,
    app::{AppDataError, AppDataResponse, RgClientPayload, RgClientPayloadResponse, RgClientPayloadError},
    networking::{
        Network,
        network::peers::OutboundPeerRequest,
    },
    proto::peer,
    utils,
};

/// The default timeout for forwarded client requests.
const DEFAULT_FORWARDING_TIMEOUT: Duration = Duration::from_secs(5);
const ERR_MAILBOX_DURING_FORWARDED_REQUEST: &str = "Encountered an actix MailboxError while handling a forwarded client request.";

//////////////////////////////////////////////////////////////////////////////////////////////////
// ForwardToLeader ///////////////////////////////////////////////////////////////////////////////

/// A request from a client actor to forward the given payload to the leader.
pub(in crate::networking) struct ForwardToLeader {
    payload: RgClientPayload,
    leader_opt: Option<NodeId>,
}

impl ForwardToLeader {
    /// Construct a new instance.
    pub fn new(payload: RgClientPayload, leader_opt: Option<NodeId>) -> Self {
        Self{payload, leader_opt}
    }
}

impl Message for ForwardToLeader {
    type Result = Result<AppDataResponse, AppDataError>;
}

impl Handler<ForwardToLeader> for Network {
    type Result = ResponseActFuture<Self, AppDataResponse, AppDataError>;

    fn handle(&mut self, msg: ForwardToLeader, ctx: &mut Self::Context) -> Self::Result {
        Box::new(self.handle_forwarding_request(msg, ctx))
    }
}

impl Network {
    /// Forward any buffered client requests.
    pub(super) fn forward_buffered_client_requests(&mut self, ctx: &mut Context<Self>) {
        let leader = match self.current_leader {
            Some(leader) => leader,
            None => return,
        };
        let forwarding_buf: Vec<_> = self.forwarding_buffer.drain().collect();
        for (_, (mut payload, tx)) in forwarding_buf {
            payload.leader_opt = Some(leader);
            let f = self.handle_forwarding_request(payload, ctx)
                .then(move |res, _: &mut Self, _| -> FutureResult<(), (), Self> {
                    let _ = tx.send(res);
                    fut::ok(())
                });
            ctx.spawn(f);
        }
    }

    fn handle_forwarding_request(&mut self, msg: ForwardToLeader, ctx: &mut Context<Self>) -> impl ActorFuture<Actor=Self, Item=AppDataResponse, Error=AppDataError> {
        // If Raft has given us an ID, attempt to forward the request immediately, else buffer.
        match msg.leader_opt {
            Some(target_node) => {
                let data = utils::bin_encode_client_payload(&msg.payload);
                let request = peer::Request::new_forwarded(data);
                let outbound = OutboundPeerRequest{request, target_node, timeout: DEFAULT_FORWARDING_TIMEOUT};
                fut::Either::A(fut::wrap_future(self.send_outbound_peer_request(outbound, ctx))
                    .then(|res, act: &mut Self, _| act.handle_forwarding_response(res)))
            },
            None => {
                let (tx, rx) = oneshot::channel();
                let req_id = Uuid::new_v4().to_string();
                self.forwarding_buffer.insert(req_id.clone(), (msg, tx));
                fut::Either::B(fut::wrap_future(rx)
                    .map_err(|err, _, _| {
                        log::error!("Forwarded request was unexpectedly cancelled. {}", err);
                        AppDataError::Internal
                    })
                    .timeout(DEFAULT_FORWARDING_TIMEOUT, AppDataError::ForwardingTimeout)
                    .then(move |res, act: &mut Self, _| match res {
                        Ok(inner_res) => fut::result(inner_res),
                        Err(err) => {
                            let _ = act.forwarding_buffer.remove(&req_id);
                            fut::err(err)
                        }
                    }))
            }
        }
    }

    /// Unpack the response from the forwarded client payload.
    ///
    /// This is the final stage in any forwarding chain. When the forwarded request chain fully
    /// unwinds, it will end up here, where the contents of the encapsulated data are deserialized
    /// and prepped for response to the original requesting client.
    fn handle_forwarding_response(&mut self, res: Result<peer::Response, ()>) -> impl ActorFuture<Actor=Self, Item=AppDataResponse, Error=AppDataError> {
        match res {
            Ok(res) => match res.segment {
                Some(peer::response::Segment::Error(err)) => {
                    let peer_err = peer::Error::from_i32(err);
                    log::error!("Error during client request forwarding. {:?}", peer_err);
                    match peer_err {
                        Some(peer::Error::Timeout) => fut::err(AppDataError::ForwardingTimeout),
                        _ => fut::err(AppDataError::Internal),
                    }
                },
                Some(peer::response::Segment::Forwarded(forwarded)) => match forwarded.result {
                    Some(peer::forwarded_client_response::Result::Data(data_buf)) => fut::result(utils::bin_decode_app_data_response_as_result(data_buf)),
                    Some(peer::forwarded_client_response::Result::Error(err_buf)) => fut::err(utils::bin_decode_app_data_error(err_buf)),
                    None => {
                        log::error!("Received a forwarding response body with an unknown frame variant.");
                        fut::err(AppDataError::Internal)
                    }
                }
                _ => {
                    log::error!("Received a forwarding response with an unexpected.");
                    fut::err(AppDataError::Internal)
                }
            },
            Err(_) => {
                log::error!("Forwarding response handler unexpectedly hit an error branch, this should not happen.");
                fut::err(AppDataError::Internal)
            }
        }
    }
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// ForwardedClientRequest ////////////////////////////////////////////////////////////////////////

/// A request to handle a forwarded client payload.
pub(in crate::networking) struct ForwardedRequest(RgClientPayload);

impl ForwardedRequest {
    /// Construct a new instance.
    pub fn new(inner: RgClientPayload) -> Self {
        Self(inner)
    }
}

impl Message for ForwardedRequest {
    type Result = Result<AppDataResponse, AppDataError>;
}

impl Handler<ForwardedRequest> for Network {
    type Result = ResponseActFuture<Self, AppDataResponse, AppDataError>;

    /// NOTE: with the current implementation, we will pay a compounding cost every time the frame
    /// is forwarded. However, an overwhelming majority of the time, client requests will need to
    /// be forwarded only once.
    fn handle(&mut self, msg: ForwardedRequest, _: &mut Self::Context) -> Self::Result {
        Box::new(fut::wrap_future(self.services.client_payload.send(msg.0)
            .map_err(|err| utils::client_error_from_mailbox_error(err, ERR_MAILBOX_DURING_FORWARDED_REQUEST))
            .and_then(|res| res))
            .then(|res, act: &mut Self, ctx| act.unpack_client_payload_app_data(res, ctx)))
    }
}

impl Network {
    /// Unpack the given client payload result, and attempt to extract its applied app data.
    ///
    /// Error results will be transformed into API ClientErrors, message forwarding will be
    /// performed as needed, and payloads which have only been committed (vs applied) will cause
    /// an error to be returned.
    fn unpack_client_payload_app_data(
        &mut self, res: Result<RgClientPayloadResponse, RgClientPayloadError>, ctx: &mut Context<Self>,
    ) -> impl ActorFuture<Actor=Self, Item=AppDataResponse, Error=AppDataError> {
        match res {
            Err(err) => match err {
                RgClientPayloadError::Internal => fut::Either::A(fut::err(AppDataError::Internal)),
                RgClientPayloadError::Application(app_err) => fut::Either::A(fut::err(app_err)),
                RgClientPayloadError::ForwardToLeader{payload: req, leader} => {
                    fut::Either::B(self.handle_forwarding_request(ForwardToLeader::new(req, leader), ctx))
                },
            },
            Ok(payload) => match payload {
                RgClientPayloadResponse::Committed{..} => {
                    log::error!("Received a Committed payload response from Raft, expected Applied. Internal error.");
                    fut::Either::A(fut::err(AppDataError::Internal))
                }
                RgClientPayloadResponse::Applied{data, ..} => fut::Either::A(fut::ok(data)),
            }
        }
    }
}
