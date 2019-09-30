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
use bytes;
use log::{debug, error, info, warn};
use uuid;

use crate::{
    NodeId,
    auth::{Claims},
    proto::client::api::{
        self, ClientError,
        client_frame::Payload as ClientPayload,
        server_frame::Payload as ServerPayload,
    },
};

//////////////////////////////////////////////////////////////////////////////////////////////////
// WsClientServices //////////////////////////////////////////////////////////////////////////////

/// All services needed by the `WsClient` actor.
pub(super) struct WsClientServices {
    verify_token: Recipient<VerifyToken>,
}

impl WsClientServices {
    /// Create a new instance.
    pub fn new(verify_token: Recipient<VerifyToken>) -> Self {
        Self{verify_token}
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
    _claims: Claims,
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

    /// Handle client `ConnectRequest` frame.
    fn handle_connect(&mut self, frame: api::ConnectRequest, meta: api::FrameMeta, ctx: &mut ws::WebsocketContext<Self>) {
        // Issue a normal response if the connection is already active.
        if let ClientState::Active(_) = &self.state {
            warn!("Client {} sent a ConnectRequest even though the connection state is active.", self.connection_id);
            return self.send_frame(ServerPayload::Connect(api::ConnectResponse{error: None, id: self.connection_id.clone()}), meta, ctx);
        }

        // Call network service to validate the given token and extract its claims object.
        let f = fut::wrap_future(self.services.verify_token.send(VerifyToken(frame.token)))
            .map_err(|_, _: &mut Self, _| ClientError::new_internal())
            .and_then(|res, _, _| fut::result(res))

            // Transition to active state.
            .and_then(|_claims, act, _| {
                act.state = ClientState::Active(ClientStateActive{_claims});
                fut::ok(())
            })

            // Emit response.
            .then(move |res, act, ctx| {
                match res {
                    Ok(_) => act.send_frame(ServerPayload::Connect(api::ConnectResponse{error: None, id: act.connection_id.clone()}), meta, ctx),
                    Err(err) => act.send_frame(ServerPayload::Connect(api::ConnectResponse{error: Some(err), id: String::new()}), meta, ctx),
                }
                fut::ok(())
            });

        ctx.spawn(f);
    }

    /// Perform a healthcheck at the given interval.
    fn healthcheck(&mut self, ctx: &mut ws::WebsocketContext<Self>) {
        if let Some(handle) = self.heartbeat_handle.take() {
            ctx.cancel_future(handle);
        }
        self.heartbeat_handle = Some(ctx.run_interval(self.liveness_threshold, |act, ctx| {
            if Instant::now().duration_since(act.heartbeat) > act.liveness_threshold {
                info!("Client connection {} appears to be dead, disconnecting.", act.connection_id);
                ctx.stop();
            }
        }));
    }

    /// Send a frame to the connected client.
    fn send_frame(&mut self, payload: ServerPayload, meta: api::FrameMeta, ctx: &mut ws::WebsocketContext<Self>) {
        use prost::Message;
        let frame = api::ServerFrame{payload: Some(payload), meta: Some(meta)};
        let mut data = bytes::BytesMut::with_capacity(frame.encoded_len());
        let _ = frame.encode(&mut data).map_err(|err| error!("Failed to serialize protobuf frame. {}", err));
        ctx.binary(data);
    }
}

impl Actor for WsClient {
    type Context = ws::WebsocketContext<Self>;

    /// Logic for starting this actor.
    ///
    /// Clients are responsible for driving the heartbeat / healtcheck system. The server will
    /// simply check for liveness at the configured interval.
    fn started(&mut self, ctx: &mut Self::Context) {
        self.healthcheck(ctx);
    }
}

impl StreamHandler<ws::Message, ws::ProtocolError> for WsClient {
    /// Handle messages received over the WebSocket.
    fn handle(&mut self, msg: ws::Message, ctx: &mut Self::Context) {
        match msg {
            ws::Message::Nop => (),
            ws::Message::Close(reason) => debug!("Connection with client {} is closing. {:?}", self.connection_id, reason),
            ws::Message::Ping(_) => {
                // Heartbeat response received from connected client.
                self.heartbeat = Instant::now();
            }
            ws::Message::Pong(_) => warn!("Protocol error. Unexpectedly received a pong frame from connected client."),
            ws::Message::Text(_) => warn!("Protocol error. Unexpectedly received a text frame from connected client."),
            ws::Message::Binary(data) => {
                // Decode the received frame.
                debug!("Handling frame from connected client.");
                use prost::Message;
                let frame = match api::ClientFrame::decode(data) {
                    Ok(frame) => frame,
                    Err(err) => {
                        error!("Error decoding binary frame from client connection {}. {}", self.connection_id, err);
                        return;
                    }
                };

                // If the frame is a response frame, route it through to its matching request.
                let meta = frame.meta.unwrap_or_default();
                match frame.payload {
                    Some(ClientPayload::Connect(payload)) => self.handle_connect(payload, meta, ctx),
                    Some(ClientPayload::Disconnect(_payload)) => (), // self.handler(payload, meta, ctx),
                    Some(ClientPayload::PubEphemeral(_payload)) => (), // self.handler(payload, meta, ctx),
                    Some(ClientPayload::PubRpc(_payload)) => (), // self.handler(payload, meta, ctx),
                    Some(ClientPayload::PubStream(_payload)) => (), // self.handler(payload, meta, ctx),
                    Some(ClientPayload::SubEphemeral(_payload)) => (), // self.handler(payload, meta, ctx),
                    Some(ClientPayload::SubRpc(_payload)) => (), // self.handler(payload, meta, ctx),
                    Some(ClientPayload::SubStream(_payload)) => (), // self.handler(payload, meta, ctx),
                    Some(ClientPayload::SubPipeline(_payload)) => (), // self.handler(payload, meta, ctx),
                    Some(ClientPayload::UnsubStream(_payload)) => (), // self.handler(payload, meta, ctx),
                    Some(ClientPayload::UnsubPipeline(_payload)) => (), // self.handler(payload, meta, ctx),
                    Some(ClientPayload::EnsureEndpoint(_payload)) => (), // self.handler(payload, meta, ctx),
                    Some(ClientPayload::EnsureStream(_payload)) => (), // self.handler(payload, meta, ctx),
                    Some(ClientPayload::EnsurePipeline(_payload)) => (), // self.handler(payload, meta, ctx),
                    Some(ClientPayload::AckStream(_payload)) => (), // self.handler(payload, meta, ctx),
                    Some(ClientPayload::AckPipeline(_payload)) => (), // self.handler(payload, meta, ctx),
                    None => {
                        warn!("Empty or unrecognized client frame payload received on connection {}.", self.connection_id);
                    }
                }
            }
        }
    }
}
