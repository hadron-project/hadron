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

use actix::{
    prelude::*,
    dev::ToEnvelope,
};
use actix_web_actors::ws;
use log::{debug, error, info, warn};
use uuid;

use crate::{
    NodeId,
    networking::{Network},
    proto::client::api::{
        self, ClientError,
        client_frame::Payload as ClientPayload,
        server_frame::Payload as ServerPayload,
    },
};

//////////////////////////////////////////////////////////////////////////////////////////////////
// WsClientProvider //////////////////////////////////////////////////////////////////////////////

/// The `WsClient` actor's provider interface.
pub(super) trait WsClientProvider: 'static {
    /// The type to use as the provider. Should just be `Self` of the implementing type.
    type Actor: Actor<Context=Self::Context> + Handler<CheckTokenLiveness>;

    /// The type to use as the storage actor's context. Should be `Context<Self>` or `SyncContext<Self>`.
    type Context: ActorContext + ToEnvelope<Self::Actor, CheckTokenLiveness>;
}

/// Check if the given token is still valid based on its ID.
pub(super) struct CheckTokenLiveness(pub String);

impl Message for CheckTokenLiveness {
    type Result = Result<(), ()>;
}

impl WsClientProvider for Network {
    type Actor = Self;
    type Context = Context<Self>;
}

impl Handler<CheckTokenLiveness> for Network {
    type Result = ResponseActFuture<Self, (), ()>;

    fn handle(&mut self, _msg: CheckTokenLiveness, _ctx: &mut Context<Self>) -> Self::Result {
        // TODO: need to finish this up.
        Box::new(fut::ok(()))
    }
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// ClientState ///////////////////////////////////////////////////////////////////////////////////

enum ClientState {
    Initial,
    Active(ClientStateActive),
}

struct ClientStateActive; // TODO: add permissions cache here.

//////////////////////////////////////////////////////////////////////////////////////////////////
// WsClient //////////////////////////////////////////////////////////////////////////////////////

/// An actor responsible for handling inbound WebSocket connections from clients.
pub(super) struct WsClient<P: WsClientProvider> {
    /// The ID of this node.
    node_id: NodeId,

    /// The address of the provider.
    provider: Addr<P::Actor>,

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

    // /// A map of pending stream deliveries awaiting a client response.
    // stream_outbound: HashMap<String, (oneshot::Sender<Result<api::Response, ()>>, SpawnHandle)>, // TODO: update type.
}

impl<P: WsClientProvider> WsClient<P> {
    /// Create a new instance.
    pub fn new(provider: Addr<P::Actor>, node_id: NodeId, liveness_threshold: Duration) -> Self {
        Self{
            node_id, provider,
            connection_id: uuid::Uuid::new_v4().to_string(),
            heartbeat: Instant::now(),
            heartbeat_handle: None,
            state: ClientState::Initial,
            liveness_threshold,
        }
    }

    // /// Sever the connection with the peer after sending a disconnect frame.
    // fn disconnect(&mut self, disconnect: api::Disconnect, ctx: &mut ws::WebsocketContext<Self>) {
    //     debug!("Peer connection needs disconnect: {}. Closing.", disconnect as i32);
    //     use prost::Message;
    //     let frame = api::Frame{meta: None, payload: Some(api::frame::Payload::Disconnect(disconnect as i32))};
    //     let mut data = bytes::BytesMut::with_capacity(frame.encoded_len());
    //     let _ = frame.encode(&mut data).map_err(|err| error!("Failed to serialize protobuf frame. {}", err));
    //     ctx.binary(data);
    //     ctx.stop();
    // }

    /// Handle client `ConnectRequest` frame.
    ///
    /// A handshake frame has been received. Update the peer ID based on the given information,
    /// propagate all of this information to the parent `Network` actor, and then response to
    /// the caller with a handshake frame as well.
    fn handle_connect(&mut self, frame: api::ConnectRequest, meta: api::FrameMeta, ctx: &mut ws::WebsocketContext<Self>) {
        // Issue a normal response if the connection is already active.
        if let ClientState::Active(_) = &self.state {
            warn!("Client {} sent a ConnectRequest even though the connection state is active.", self.connection_id);
            return self.send_frame(ServerPayload::Connect(api::ConnectResponse{error: None, id: self.connection_id.clone()}), meta, ctx);
        }

        // Extract auth data and statically validate it.
        // TODO: implement this. See #24.
        if &frame.token != "" {
            return self.send_frame(ServerPayload::Connect(api::ConnectResponse{
                error: Some(ClientError::new_unauthorized()),
                id: String::new(),
            }), meta, ctx);
        }

        // Call provider to ensure the client's token ID is still live.
        let f = fut::wrap_future(self.provider.send(CheckTokenLiveness(String::new())))
            .map_err(|_, _, _| ()).then(|res, _, _| fut::result(res))
            .and_then(|_, _, _| fut::ok(()));

        // TODO:
        // - Register the client connection and its auth data with the provider.
        // - Transition to active state.

        ctx.spawn(f);
    }

    // /// Handle sending a handshake response.
    // fn handshake_response(&mut self, meta: api::Meta, ctx: &mut ws::WebsocketContext<Self>) {
    //     // Respond to the caller with a handshake frame.
    //     // TODO: finish up the routing info pattern. See the peer connection management doc.
    //     use prost::Message;
    //     let hs_out = api::Handshake{node_id: self.node_id, routing_info: String::with_capacity(0)};
    //     let frame = api::Frame{meta: Some(meta), payload: Some(api::frame::Payload::Response(api::Response{
    //         segment: Some(api::response::Segment::Handshake(hs_out)),
    //     }))};
    //     let mut data = bytes::BytesMut::with_capacity(frame.encoded_len());
    //     let _ = frame.encode(&mut data).map_err(|err| error!("Failed to serialize protobuf frame. {}", err));
    //     ctx.binary(data);
    // }

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

    fn send_frame(&mut self, payload: ServerPayload, meta: api::FrameMeta, ctx: &mut ws::WebsocketContext<Self>) {

    }
}

impl<P: WsClientProvider> Actor for WsClient<P> {
    type Context = ws::WebsocketContext<Self>;

    /// Logic for starting this actor.
    ///
    /// Clients are responsible for driving the heartbeat / healtcheck system. The server will
    /// simply check for liveness at the configured interval.
    fn started(&mut self, ctx: &mut Self::Context) {
        self.healthcheck(ctx);
    }
}

impl<P: WsClientProvider> StreamHandler<ws::Message, ws::ProtocolError> for WsClient<P> {
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
