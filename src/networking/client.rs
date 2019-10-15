//! A module encapsulating the `WsClient` actor and its logic.
//!
//! The `WsClient` actor represents a WebSocket connection which was initialized by a client. The
//! client may be written in any number of different languages, but all clients must adhere to the
//! Railgun Client Wire Protocol in order to successfully communicate with the cluster.
//!
//! Client requests may be forwarded along to other members of the cluster as needed in order
//! to satisfy the clients request.

use std::{
    time::{Duration, Instant},
};

use actix::prelude::*;
use actix_web_actors::ws;
use actix_raft::messages::{ResponseMode};
use bytes;
use log;
use uuid;

use crate::{
    NodeId,
    app::{AppData, AppDataError, AppDataResponse, RgEntryNormal, RgClientPayload, RgClientPayloadError, RgClientPayloadResponse},
    auth::{Claims},
    proto::client::{
        self, ClientError, FrameMeta,
        client_frame::Payload as ClientFramePayload,
        server_frame::Payload as ServerFramePayload,
    },
    utils,
};

const ERR_MAILBOX_DURING_CLIENT_REQUEST: &str = "Encountered an actix MailboxError while handling a client request.";
type WsClientCtx = ws::WebsocketContext<WsClient>;

//////////////////////////////////////////////////////////////////////////////////////////////////
// WsClientServices //////////////////////////////////////////////////////////////////////////////

/// All services needed by the `WsClient` actor.
pub(super) struct WsClientServices {
    client_payload: Recipient<RgClientPayload>,
    verify_token: Recipient<VerifyToken>,
}

impl WsClientServices {
    /// Create a new instance.
    pub fn new(client_payload: Recipient<RgClientPayload>, verify_token: Recipient<VerifyToken>) -> Self {
        Self{client_payload, verify_token}
    }
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// VerifyToken ///////////////////////////////////////////////////////////////////////////////////

/// Check if the given token is still valid based on its ID.
pub(super) struct VerifyToken(pub String);

impl Message for VerifyToken {
    type Result = Result<Claims, ClientError>;
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// ClientState ///////////////////////////////////////////////////////////////////////////////////

enum ClientState {
    Initial,
    Active(ClientStateActive),
}

/// The state associated with an active client connection.
///
/// TODO: add permissions cache here.
struct ClientStateActive {
    /// The authN/authZ claims of the connection.
    claims: Claims,
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// WsClient //////////////////////////////////////////////////////////////////////////////////////

/// An actor responsible for handling inbound WebSocket connections from clients.
pub(super) struct WsClient {
    /// The ID of this node.
    _node_id: NodeId,
    /// The services which this actor depends upon.
    services: WsClientServices,
    /// The ID assigned to this connection.
    connection_id: String,
    /// The last successful heartbeat on this socket. Will reckon the client as being dead after
    /// `CLIENT_HB_THRESHOLD` has been exceeded since last successful heartbeat.
    heartbeat: Instant,
    /// A handle to the heartbeat interval job.
    heartbeat_handle: Option<SpawnHandle>,
    /// The state of the client connection.
    state: ClientState,
    /// The configured liveness threshold for this client connection.
    ///
    /// If this amount of time elapses without hearing from the client, it will be reckoned dead.
    /// This value may be updated by clients during the handshake.
    liveness_threshold: Duration,
}

impl WsClient {
    /// Create a new instance.
    pub fn new(services: WsClientServices, _node_id: NodeId, liveness_threshold: Duration) -> Self {
        Self{
            _node_id, services,
            connection_id: uuid::Uuid::new_v4().to_string(),
            heartbeat: Instant::now(),
            heartbeat_handle: None,
            state: ClientState::Initial,
            liveness_threshold,
        }
    }

    /// Perform a healthcheck at the given interval.
    fn healthcheck(&mut self, ctx: &mut WsClientCtx) {
        if let Some(handle) = self.heartbeat_handle.take() {
            ctx.cancel_future(handle);
        }
        log::debug!("Starting client healthcheck loop with liveness threshold {:?}", self.liveness_threshold);
        self.heartbeat_handle = Some(ctx.run_interval(self.liveness_threshold, |act, ctx| {
            log::debug!("Running healthcheck loop on client {}.", &act.connection_id);
            if Instant::now().duration_since(act.heartbeat) > act.liveness_threshold {
                log::info!("Client connection {} appears to be dead, disconnecting.", act.connection_id);
                ctx.stop();
            }
        }));
    }

    /// Forward the given client request to the cluster's current leader.
    fn forward_to_leader(
        &mut self, _req: RgClientPayload, meta: FrameMeta, _leader: Option<NodeId>, _ctx: &mut WsClientCtx,
    ) -> impl ActorFuture<Actor=Self, Item=(FrameMeta, AppDataResponse), Error=(FrameMeta, AppDataError)> {
        // NOTE/TODO: this is just stubbed logic for now.
        log::error!("Forwarding client requests to cluster leader is not yet implemented.");
        fut::err((meta, AppDataError::Internal)) // TODO: finish this up.
    }

    /// Send a frame to the connected client.
    fn send_frame(&mut self, payload: ServerFramePayload, meta: FrameMeta, ctx: &mut WsClientCtx) {
        use prost::Message;
        let frame = client::ServerFrame{payload: Some(payload), meta: Some(meta)};
        let mut data = bytes::BytesMut::with_capacity(frame.encoded_len());
        let _ = frame.encode(&mut data).map_err(|err| log::error!("Failed to serialize protobuf frame. {}", err));
        ctx.binary(data);
    }

    /// Unpack the given client payload result, and attempt to extract its applied app data.
    ///
    /// Error results will be transformed into API ClientErrors, message forwarding will be
    /// performed as needed, and payloads which have only been committed (vs applied) will cause
    /// an error to be returned.
    fn unpack_client_payload_app_data(
        &mut self, res: Result<RgClientPayloadResponse, RgClientPayloadError>, meta: FrameMeta, ctx: &mut WsClientCtx,
    ) -> impl ActorFuture<Actor=Self, Item=(FrameMeta, AppDataResponse), Error=(FrameMeta, AppDataError)> {
        match res {
            Err(err) => match err {
                RgClientPayloadError::Internal => fut::Either::A(fut::err((meta, AppDataError::Internal))),
                RgClientPayloadError::Application(app_err) => fut::Either::A(fut::err((meta, app_err))),
                RgClientPayloadError::ForwardToLeader{payload: req, leader} => return fut::Either::B(self.forward_to_leader(req, meta, leader, ctx)),
            },
            Ok(payload) => match payload {
                RgClientPayloadResponse::Committed{..} => {
                    log::error!("Received a Committed payload response from Raft, expected Applied. Internal error.");
                    fut::Either::A(fut::err((meta, AppDataError::Internal)))
                }
                RgClientPayloadResponse::Applied{data, ..} => fut::Either::A(fut::ok((meta, data))),
            }
        }
    }

    //////////////////////////////////////////////////////////////////////////
    // Request Handlers //////////////////////////////////////////////////////

    /// Handle client `ConnectRequest` frame.
    fn handle_connect(&mut self, frame: client::ConnectRequest, meta: FrameMeta, ctx: &mut WsClientCtx) {
        // Issue a normal response if the connection is already active.
        if let ClientState::Active(_) = &self.state {
            log::warn!("Client {} sent a ConnectRequest even though the connection state is active.", self.connection_id);
            return self.send_frame(ServerFramePayload::Connect(client::ConnectResponse::new(self.connection_id.clone())), meta, ctx);
        }

        // Call network service to validate the given token and extract its claims object.
        let liveness_threshold = Duration::from_secs(frame.liveness_threshold as u64);
        let f = fut::wrap_future(self.services.verify_token.send(VerifyToken(frame.token)))
            .map_err(|_, _: &mut Self, _| ClientError::new_internal())
            .and_then(|res, _, _| fut::result(res))

            // Transition to active state.
            .and_then(move |claims, act, ctx| {
                act.state = ClientState::Active(ClientStateActive{claims});
                act.liveness_threshold = liveness_threshold;
                act.healthcheck(ctx);
                fut::ok(())
            })

            // Emit response.
            .then(move |res, act, ctx| {
                match res {
                    Ok(_) => act.send_frame(ServerFramePayload::Connect(client::ConnectResponse::new(act.connection_id.clone())), meta, ctx),
                    Err(err) => act.send_frame(ServerFramePayload::Connect(client::ConnectResponse::err(err)), meta, ctx),
                }
                fut::ok(())
            });

        ctx.spawn(f);
    }

    /// Handle client `EnsureStream` frame.
    fn handle_ensure_stream(&mut self, req: client::EnsureStreamRequest, meta: FrameMeta, ctx: &mut WsClientCtx) {
        // Ensure client is in an active state.
        let state = match &self.state {
            ClientState::Initial => return self.send_frame(ServerFramePayload::EnsureStream(client::EnsureStreamResponse::new_err(ClientError::new_handshake_required())), meta, ctx),
            ClientState::Active(state) => state,
        };

        // Ensure client is authorized to publish to the target stream.
        if let Err(err) = state.claims.check_ensure_stream_auth(&req) {
            return self.send_frame(ServerFramePayload::EnsureStream(client::EnsureStreamResponse::new_err(err)), meta, ctx);
        }

        // Everything checks out, so send the request over to Raft.
        let data = AppData::EnsureStream(req);
        let f = fut::wrap_future(self.services.client_payload.send(RgClientPayload::new(RgEntryNormal{data}, ResponseMode::Applied))
            .map_err(|err| utils::client_error_from_mailbox_error(err, ERR_MAILBOX_DURING_CLIENT_REQUEST))
            .and_then(|res| res)).then(move |res, act: &mut Self, ctx| act.unpack_client_payload_app_data(res, meta, ctx))
            .then(|res, act, ctx| act.send_ensure_stream_response(res, ctx));
        ctx.spawn(f);
    }

    /// Handle client `PubStreamRequest` frame.
    fn handle_pub_stream(&mut self, req: client::PubStreamRequest, meta: FrameMeta, ctx: &mut WsClientCtx) {
        // Ensure client is in an active state.
        let state = match &self.state {
            ClientState::Initial => return self.send_frame(ServerFramePayload::PubStream(client::PubStreamResponse::new_err(ClientError::new_handshake_required())), meta, ctx),
            ClientState::Active(state) => state,
        };

        // Ensure client is authorized to publish to the target stream.
        if let Err(err) = state.claims.check_stream_pub_auth(&req) {
            return self.send_frame(ServerFramePayload::PubStream(client::PubStreamResponse::new_err(err)), meta, ctx);
        }

        // Everything checks out, so send the request over to Raft.
        let data = AppData::PubStream(req);
        let f = fut::wrap_future(self.services.client_payload.send(RgClientPayload::new(RgEntryNormal{data}, ResponseMode::Applied))
            .map_err(|err| utils::client_error_from_mailbox_error(err, ERR_MAILBOX_DURING_CLIENT_REQUEST))
            .and_then(|res| res)).then(move |res, act: &mut Self, ctx| act.unpack_client_payload_app_data(res, meta, ctx))
            .then(|res, act, ctx| act.send_pub_stream_response(res, ctx));
        ctx.spawn(f);
    }

    //////////////////////////////////////////////////////////////////////////
    // Response Handlers /////////////////////////////////////////////////////

    /// Send EnsureStream response.
    fn send_ensure_stream_response(&mut self, res: Result<(FrameMeta, AppDataResponse), (FrameMeta, AppDataError)>, ctx: &mut WsClientCtx) -> impl ActorFuture<Actor=Self, Item=(), Error=()> {
        let (meta, res) = match res {
            Err((meta, err)) => match err {
                AppDataError::TargetStreamExists => (meta, ServerFramePayload::EnsureStream(client::EnsureStreamResponse::new())),
                _ => (meta, ServerFramePayload::EnsureStream(client::EnsureStreamResponse::new_err(ClientError::from(err)))),
            }
            Ok((meta, data)) => match data {
                AppDataResponse::EnsureStream => (meta, ServerFramePayload::EnsureStream(client::EnsureStreamResponse::new())),
                _ => {
                    log::error!("Expected an EnsureStream data response from Raft, got something else. Internal error.");
                    (meta, ServerFramePayload::EnsureStream(client::EnsureStreamResponse::new_err(ClientError::new_internal())))
                }
            }
        };
        self.send_frame(res, meta, ctx);
        fut::ok(())
    }

    /// Send PubStream response.
    fn send_pub_stream_response(&mut self, res: Result<(FrameMeta, AppDataResponse), (FrameMeta, AppDataError)>, ctx: &mut WsClientCtx) -> impl ActorFuture<Actor=Self, Item=(), Error=()> {
        let (meta, res) = match res {
            Err((meta, err)) => (meta, ServerFramePayload::PubStream(client::PubStreamResponse::new_err(ClientError::from(err)))),
            Ok((meta, data)) => match data {
                AppDataResponse::PubStream{index} => (meta, ServerFramePayload::PubStream(client::PubStreamResponse::new(index))),
                _ => {
                    log::error!("Expected a PubStream data response from Raft, got something else. Internal error.");
                    (meta, ServerFramePayload::PubStream(client::PubStreamResponse::new_err(ClientError::new_internal())))
                }
            }
        };
        self.send_frame(res, meta, ctx);
        fut::ok(())
    }
}

impl Actor for WsClient {
    type Context = ws::WebsocketContext<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        self.healthcheck(ctx);
    }

    fn stopped(&mut self, _: &mut Self::Context) {
        log::debug!("Client {} is disconnecting.", &self.connection_id);
    }
}

impl StreamHandler<ws::Message, ws::ProtocolError> for WsClient {
    /// Handle messages received over the WebSocket.
    fn handle(&mut self, msg: ws::Message, ctx: &mut Self::Context) {
        match msg {
            ws::Message::Nop => (),
            ws::Message::Close(reason) => log::debug!("Connection with client {} is closing. {:?}", self.connection_id, reason),
            ws::Message::Ping(_) => {
                self.heartbeat = Instant::now();
                ctx.pong("");
            }
            ws::Message::Pong(_) => log::warn!("Protocol error. Unexpectedly received a pong frame from connected client."),
            ws::Message::Text(_) => log::warn!("Protocol error. Unexpectedly received a text frame from connected client."),
            ws::Message::Binary(data) => {
                // Decode the received frame.
                log::debug!("Handling frame from connected client.");
                use prost::Message;
                let frame = match client::ClientFrame::decode(data) {
                    Ok(frame) => frame,
                    Err(err) => {
                        log::error!("Error decoding binary frame from client connection {}. {}", self.connection_id, err);
                        return;
                    }
                };
                self.heartbeat = Instant::now();

                // If the frame is a response frame, route it through to its matching request.
                let meta = frame.meta.unwrap_or_default();
                match frame.payload {
                    Some(ClientFramePayload::Connect(payload)) => self.handle_connect(payload, meta, ctx),
                    Some(ClientFramePayload::Disconnect(_payload)) => (), // self.handler(payload, meta, ctx),
                    Some(ClientFramePayload::PubEphemeral(_payload)) => (), // self.handler(payload, meta, ctx),
                    Some(ClientFramePayload::PubRpc(_payload)) => (), // self.handler(payload, meta, ctx),
                    Some(ClientFramePayload::PubStream(payload)) => self.handle_pub_stream(payload, meta, ctx),
                    Some(ClientFramePayload::SubEphemeral(_payload)) => (), // self.handler(payload, meta, ctx),
                    Some(ClientFramePayload::SubRpc(_payload)) => (), // self.handler(payload, meta, ctx),
                    Some(ClientFramePayload::SubStream(_payload)) => (), // self.handler(payload, meta, ctx),
                    Some(ClientFramePayload::SubPipeline(_payload)) => (), // self.handler(payload, meta, ctx),
                    Some(ClientFramePayload::UnsubStream(_payload)) => (), // self.handler(payload, meta, ctx),
                    Some(ClientFramePayload::UnsubPipeline(_payload)) => (), // self.handler(payload, meta, ctx),
                    Some(ClientFramePayload::EnsureEndpoint(_payload)) => (), // self.handler(payload, meta, ctx),
                    Some(ClientFramePayload::EnsureStream(payload)) => self.handle_ensure_stream(payload, meta, ctx),
                    Some(ClientFramePayload::EnsurePipeline(_payload)) => (), // self.handler(payload, meta, ctx),
                    Some(ClientFramePayload::AckStream(_payload)) => (), // self.handler(payload, meta, ctx),
                    Some(ClientFramePayload::AckPipeline(_payload)) => (), // self.handler(payload, meta, ctx),
                    None => {
                        log::warn!("Empty or unrecognized client frame payload received on connection {}.", self.connection_id);
                    }
                }
            }
        }
    }
}
