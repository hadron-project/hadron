//! A module encapsulating the `WsToPeer` actor and its logic.
//!
//! The `WsToPeer` actor represents a WebSocket connection which was initialized by the source
//! node and sent to a cluster peer.

use std::{
    collections::HashMap,
    net::SocketAddr,
    time::{Duration, Instant},
};

use actix::{
    prelude::*,
    io::{WriteHandler, SinkWrite},
};
use actix_web::client::Client;
use backoff::{backoff::Backoff, ExponentialBackoff};
use futures::{
    prelude::*,
    sync::{mpsc, oneshot},
};
use awc::{
    error::{WsProtocolError},
    ws::{self, Frame, Message},
};
use log::{debug, error, warn};
use prost;

use crate::{
    app::InboundRaftRequest,
    networking::{
        PEER_HB_INTERVAL, PEER_HB_THRESHOLD, PEER_HANDSHAKE_TIMEOUT, NodeId,
        ClosingPeerConnection, DisconnectPeer, OutboundPeerRequest,
        PeerAddr, PeerConnectionIdentifier, PeerConnectionLive, PeerHandshakeState,
    },
    proto::peer::api,
    utils,
};

/// A type alias for the Sink type for outbound messages to the connected peer.
type WsSink = Box<dyn Sink<SinkItem=Message, SinkError=WsProtocolError> + 'static>;

/// A type alias for the Stream type for inbound messages from the connected peer.
type WsStream = Box<dyn Stream<Item=Frame, Error=WsProtocolError> + 'static>;

//////////////////////////////////////////////////////////////////////////////////////////////////
// WsToPeerServices //////////////////////////////////////////////////////////////////////////////

/// All services needed by the `WsToPeer` actor.
pub struct WsToPeerServices {
    pub closing_peer_connection: Recipient<ClosingPeerConnection>,
    pub inbound_raft_request: Recipient<InboundRaftRequest>,
    pub peer_connection_live: Recipient<PeerConnectionLive>,
}

impl WsToPeerServices {
    /// Create a new instance.
    pub fn new(
        closing_peer_connection: Recipient<ClosingPeerConnection>,
        inbound_raft_request: Recipient<InboundRaftRequest>,
        peer_connection_live: Recipient<PeerConnectionLive>,
    ) -> Self {
        Self{closing_peer_connection, inbound_raft_request, peer_connection_live}
    }
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// DiscoveryState ////////////////////////////////////////////////////////////////////////////////

/// A type used to keep track of an associated peer IP address's state in the discovery system.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum DiscoveryState {
    /// The peer IP is currently being observed by the discovery system.
    Observed,
    /// The peer IP has disappeared from the discovery system.
    ///
    /// When this is the case, the likelihood of reconnect failure is higher. If a reconnect does
    /// fail and the peer has disappeared from the discovery system, then reconnects should be
    /// aborted and the actor responsible for the connection should shutdown.
    Disappeared,
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// ConnectionState ///////////////////////////////////////////////////////////////////////////////

enum ConnectionState {
    Initializing,
    Reconnecting(StateReconnecting),
    Connected(StateConnected),
    Closing,
}

/// The state of this actor when it is attempting to reconnect.
struct StateReconnecting {
    backoff: ExponentialBackoff,
    retry: Option<SpawnHandle>,
}

/// The state of this actor when it is fully connected to the target peer.
struct StateConnected {
    /// The last successful heartbeat on this socket. Will reckon the peer as being dead after
    /// `PEER_HB_THRESHOLD` has been exceeded since last successful heartbeat.
    heartbeat: Instant,

    /// A handle to the currently running heartbeat loop.
    heartbeat_handle: Option<SpawnHandle>,

    /// The current outbound sink for sending data to the connected peer.
    outbound: SinkWrite<WsSink>,

    /// The handshake state of the connection.
    handshake: PeerHandshakeState,

    /// A map of all pending requests.
    requests_map: HashMap<String, (oneshot::Sender<Result<api::Response, ()>>, SpawnHandle)>,
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// WsToPeer //////////////////////////////////////////////////////////////////////////////////////

/// An actor responsible for handling inbound WebSocket connections from peers.
///
/// When a socket is received by a peer, once the connection is successfully established, no new
/// connection will be open to the same peer. The one connection will be used for communication
/// both ways until the connection is lost.
///
/// This end of a peer connection is responsible for driving the handshake protocol.
///
/// The Railgun heartbeat protcol for cluster peer connections is such that only
/// the receiving end of the connection will send pings. That is not this end.
pub(super) struct WsToPeer {
    /// The ID of this node.
    node_id: NodeId,
    /// The node ID of the connected peer.
    peer_id: Option<NodeId>,
    /// The services which this actor depends upon.
    services: WsToPeerServices,
    /// The socket address of the peer which this actor is to connect with.
    target: SocketAddr,
    /// The state of the associated socket address in the discovery system.
    discovery_state: DiscoveryState,
    /// The state of the connection with the target peer.
    connection: ConnectionState,
}

impl WsToPeer {
    /// Create a new instance.
    pub fn new(services: WsToPeerServices, node_id: NodeId, target: SocketAddr) -> Self {
        Self{
            node_id, services, target,
            peer_id: None,
            discovery_state: DiscoveryState::Observed,
            connection: ConnectionState::Initializing,
        }
    }

    /// The default backoff config to use for reconnects.
    fn backoff() -> ExponentialBackoff {
        let mut new = ExponentialBackoff::default();
        new.max_interval = Duration::from_secs(10);
        new.max_elapsed_time = None;
        new
    }

    /// Perform the Railgun peer handshake protocol with the connected peer.
    fn handshake(&mut self, ctx: &mut Context<Self>) {
        // Get a handle to the current handshake state.
        let hs_state = match &mut self.connection {
            ConnectionState::Connected(conn_state) => &mut conn_state.handshake,
            _ => return,
        };

        // Create a frame for the next state of the handshake.
        use PeerHandshakeState::*;
        let request = match hs_state {
            // TODO: finish up the routing info pattern. See the peer connection management doc.
            Initial => api::Request{segment: Some(api::request::Segment::Handshake(
                api::Handshake{node_id: self.node_id, routing_info: String::with_capacity(0)}
            ))},
            Done => return,
        };

        // Spawn the outbound request.
        let request = OutboundPeerRequest{request, target_node: 0, timeout: PEER_HANDSHAKE_TIMEOUT};
        let f = self.send_outbound_peer_request(request, ctx)
            .map(|res, act, ctx| act.handshake_response(res, ctx))
            .map_err(|_, act, ctx| act.handshake(ctx));
        ctx.spawn(f);
    }

    /// Handle a handshake response.
    fn handshake_response(&mut self, res: api::Response, ctx: &mut Context<Self>) {
        // Get a handle to the current handshake state.
        let state = match &mut self.connection {
            ConnectionState::Connected(state) => state,
            _ => return,
        };

        // Extract handshake response segment, else handle errors.
        let hs = match res.segment {
            Some(api::response::Segment::Handshake(hs)) => hs,
            Some(other) => {
                debug!("Invalid frame received from handshake request. {:?}", other);
                return self.handshake(ctx);
            }
            None => return self.handshake(ctx),
        };

        // If the connection is being made with self due to initial discovery probe, then drop the connection.
        if hs.node_id == self.node_id {
            return ctx.stop();
        }

        // If this connection already has a peer ID and has reconnected but the IDs no
        // longer match, then we need to drop this connection, as it means that the
        // node at the target IP has changed.
        if self.peer_id.is_some() && self.peer_id.as_ref() != Some(&hs.node_id) {
            return ctx.stop();
        }

        // Update handshake state & propagate routing info to parent.
        state.handshake = PeerHandshakeState::Done;
        self.peer_id = Some(hs.node_id.clone());

        // Propagate handshake info to parent `Network` actor.
        // TODO: finish up the routing info pattern. See the peer connection management doc.
        let f = self.services.peer_connection_live.send(PeerConnectionLive{
            peer_id: hs.node_id,
            routing_info: hs.routing_info,
            addr: PeerAddr::ToPeer(ctx.address()),
        });
        ctx.spawn(fut::wrap_future(f.map_err(|_| ())));
    }

    /// Healthcheck the connection on a regular interval.
    ///
    /// NOTE WELL: the Railgun heartbeat protcol for cluster peer connections is such that only
    /// the receiving end of the connection will send pings, so the initiating end just needs to
    /// check its last heartbeat on a regular interval to ensure liveness.
    fn heartbeat(&mut self, ctx: &mut Context<Self>) {
        let state = match &mut self.connection {
            ConnectionState::Connected(state) => state,
            _ => return,
        };
        state.heartbeat_handle = Some(ctx.run_interval(PEER_HB_INTERVAL, |act, ctx| {
            let interval_state = match &mut act.connection {
                ConnectionState::Connected(interval_state) => interval_state,
                _ => return,
            };
            if Instant::now().duration_since(interval_state.heartbeat) > PEER_HB_THRESHOLD {
                debug!("Peer connection appears to be dead, disconnecting.");
                act.reconnect(ctx); // Begin the reconnect process.
            }
        }));
    }

    /// Attempt to build a new connection to the target peer.
    fn new_connection(&mut self, ctx: &mut Context<Self>) {
        let (conntx, connrx) = mpsc::unbounded::<NewConnection>();
        let self_addr = ctx.address();
        actix::spawn(Client::new().ws(format!("ws://{}/internal/", &self.target)).connect()
            .then(move |res| match res {
                Ok((_httpres, framed)) => {
                    // This is a bit complex. Here we are heap allocating the new stream+sink pair
                    // from the new connection so that we can refer to them more easily. Then we
                    // are sending them to the parent via the unbounded channel.
                    let (sink, stream) = framed.split();
                    let bsink: Box<dyn Sink<SinkItem=Message, SinkError=WsProtocolError> + 'static> = Box::new(sink);
                    let bstream: Box<dyn Stream<Item=Frame, Error=WsProtocolError> + 'static> = Box::new(stream);
                    let _ = conntx.unbounded_send(NewConnection{sink: bsink, stream: bstream}); // Will never meaningfully fail.
                    Ok(())
                }
                Err(err) => {
                    // NOTE: if we need more granular error handling in the future, we will have
                    // to take a custom approach. See https://github.com/actix/actix-web/issues/838.
                    self_addr.do_send(WsClientErrorMsg(err.to_string()));
                    Ok(())
                }
            }));

        // After the above connection future is spawned, we spawn this stream to wait for
        // the returned stream & sink from a successful message. If the connection attempt
        // fails, the sending ends will be dropped and this stream will terminate.
        ctx.add_message_stream(connrx);
    }

    /// Handle the reconnect algorithm.
    fn reconnect(&mut self, ctx: &mut Context<Self>) {
        // Shut down this actor if the target peer is no longer being observed by the discovery system.
        if self.discovery_state == DiscoveryState::Disappeared {
            self.connection = ConnectionState::Closing;
            let _ = match &self.peer_id {
                Some(nodeid) => self.services.closing_peer_connection.do_send(ClosingPeerConnection(PeerConnectionIdentifier::SocketAddrAndId(self.target, nodeid.clone()))),
                None => self.services.closing_peer_connection.do_send(ClosingPeerConnection(PeerConnectionIdentifier::SocketAddr(self.target))),
            };
            return ctx.stop();
        }

        // Ensure we are in a reconnecting state.
        match &self.connection {
            ConnectionState::Reconnecting(_) => (),
            _ => self.connection = ConnectionState::Reconnecting(StateReconnecting{
                backoff: Self::backoff(),
                retry: None,
            }),
        };
        let backoffcfg = match &mut self.connection {
            ConnectionState::Reconnecting(backoffcfg) => backoffcfg,
            _ => return, // This will never be hit.
        };

        // Get the next backoff. Our config is setup such that this will never return `None`.
        let next_retry = backoffcfg.backoff.next_backoff().unwrap();
        backoffcfg.retry = Some(ctx.run_later(next_retry, |act, ctx| {
            act.new_connection(ctx);
        }));
    }

    /// Route a request over to the parent `Network` actor for handling.
    fn route_request(&mut self, req: api::Request, meta: api::Meta, ctx: &mut Context<Self>) {
        match req.segment {
            // Only this actor type sends handshake requests, so log an error if one is observed here.
            Some(api::request::Segment::Handshake(_)) => warn!("Handshake request received by a WsToPeer actor. This is a protocol violation."),
            Some(api::request::Segment::Raft(req)) => {
                let f = fut::wrap_future(self.services.inbound_raft_request.send(InboundRaftRequest(req, meta.clone())))
                    .map_err(|err, _: &mut Self, _| {
                        error!("Error propagating inbound Raft request. {}", err);
                        api::Error::Internal
                    })
                    .and_then(|res, _, _| fut::result(res))
                    .then(move |res, act, ctx| act.send_raft_response(res, meta, ctx));
                ctx.spawn(f);
            }
            None => warn!("Empty request segment received in WsToPeer."),
        }
    }

    /// Route a response payload received from the socket to its matching request future.
    fn route_response(&mut self, res: api::Response, meta: api::Meta, ctx: &mut Context<Self>) {
        let state = match &mut self.connection {
            ConnectionState::Connected(state) => state,
            _ => return,
        };

        // Extract components from request map, send the future's value & cancel its timeout.
        match state.requests_map.remove(&meta.id) {
            None => (),
            Some((tx, timeouthandle)) => {
                let _ = tx.send(Ok(res));
                ctx.cancel_future(timeouthandle);
            }
        }
    }

    /// Send a fully structured frame to the connected peer.
    fn send_frame(&mut self, frame: api::Frame, ctx: &mut Context<Self>) -> impl ActorFuture<Actor=Self, Item=(), Error=()> {
        let buf = utils::encode_peer_frame(&frame);
        self.write_outbound_message(ctx, ws::Message::Binary(buf.into()));
        fut::ok(())
    }

    /// Send the given Raft response/error result to the connected peer.
    fn send_raft_response(&mut self, res: Result<api::RaftResponse, api::Error>, meta: api::Meta, ctx: &mut Context<Self>) -> impl ActorFuture<Actor=Self, Item=(), Error=()> {
        let frame = api::Frame{
            meta: Some(meta),
            payload: Some(api::frame::Payload::Response(api::Response{
                segment: Some(match res {
                    Ok(raft_res) => api::response::Segment::Raft(raft_res),
                    Err(err) => api::response::Segment::Error(err as i32),
                }),
            })),
        };
        self.send_frame(frame, ctx)
    }

    /// Send the given outbound peer request and await a response.
    fn send_outbound_peer_request(&mut self, msg: OutboundPeerRequest, ctx: &mut Context<Self>) -> impl ActorFuture<Actor=Self, Item=api::Response, Error=()> {
        let state = match &mut self.connection {
            ConnectionState::Connected(state) => state,
            _ => return fut::Either::A(fut::err(())),
        };

        // Build the outbound request frame.
        let requestid = uuid::Uuid::new_v4().to_string();
        let frame = api::Frame{
            meta: Some(api::Meta{id: requestid.clone()}),
            payload: Some(api::frame::Payload::Request(msg.request)),
        };

        // Spawn the request's timeout handler & retain the spawnhandle.
        let closed_requestid = requestid.clone();
        let timeout = ctx.run_later(msg.timeout, move |closed_self, _closed_ctx| {
            let state = match &mut closed_self.connection {
                ConnectionState::Connected(state) => state,
                _ => return,
            };
            if let Some((_, _)) = state.requests_map.remove(&closed_requestid) {
                debug!("Request '{}' timedout.", &closed_requestid);
            }
        });

        // Create the request/response channel & add components to request map.
        let (tx, rx) = oneshot::channel();
        state.requests_map.insert(requestid, (tx, timeout));

        // Serialize the request data and write it over the outbound socket.
        let buf = utils::encode_peer_frame(&frame);
        self.write_outbound_message(ctx, ws::Message::Binary(buf.into()));

        // Return a future to the caller which will receive the response when it comes back from
        // the peer, else it will timeout.
        fut::Either::B(fut::wrap_future(rx.map_err(|err| error!("Error from OutboundPeerRequest receiver. {}", err))
            .and_then(|res| match res {
                Ok(response) => Ok(response),
                Err(_) => Err(()),
            })))
    }

    /// Write an outbound message to the connected peer.
    fn write_outbound_message(&mut self, ctx: &mut Context<Self>, msg: Message) {
        let state = match &mut self.connection {
            ConnectionState::Connected(state) => state,
            _ => return,
        };

        use futures::AsyncSink::*;
        match state.outbound.write(msg) {
            Err(err) => error!("Error while attempting to write an outbound message to peer. {:?}", err),
            Ok(asyncsink) => match asyncsink {
                Ready => (),
                NotReady(m) => {
                    error!("Could not queue message to be sent to peer.");
                    ctx.notify(RetryOutboundMessage(m));
                }
            }
        }
    }
}

impl Actor for WsToPeer {
    type Context = Context<Self>;

    /// Logic for starting this actor.
    ///
    /// This actor is initialize only with the socket addr it needs to connect to, so it is the
    /// responsibility of this actor to open the base WebSocket connection, handle reconnects,
    /// start and stop the healthcheck routine as needed, and anything else related to the
    /// lifecycle of this actor.
    fn started(&mut self, ctx: &mut Self::Context) {
        // Attempt to open a connection to the target peer.
        self.new_connection(ctx);
    }
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// UpdateDiscoveryState //////////////////////////////////////////////////////////////////////////

/// A message indicating a needed update to the tracked state of a peer in the discovery system.
///
/// This is used by the `WsToPeer` actor and it influences its reconnect behavior.
#[derive(Clone, Debug, Message)]
pub(super) struct UpdateDiscoveryState(pub DiscoveryState);

impl Handler<UpdateDiscoveryState> for WsToPeer {
    type Result = ();

    /// Handle messages to update the associated peer's state as seen by discovery system.
    fn handle(&mut self, msg: UpdateDiscoveryState, _: &mut Self::Context) {
        self.discovery_state = msg.0;
    }
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// Connection Setup //////////////////////////////////////////////////////////////////////////////

/// A message type wrapping an `awc::WsClientError`.
#[derive(Debug, Message)]
struct WsClientErrorMsg(pub String);

impl Handler<WsClientErrorMsg> for WsToPeer {
    type Result = ();

    /// An error has taken place while attempting to establish a baseline WebSocket
    /// connection to a peer.
    ///
    /// This handler is invoked only when an error takes place while attempting to open a baseline
    /// WebSocket connection to a peer. Once the WebSocket connection is established, different
    /// handlers are responsible for handling errors coming from the live connection.
    ///
    /// This handler shares the responsibility of driving reconnect logic.
    fn handle(&mut self, error: WsClientErrorMsg, ctx: &mut Self::Context) {
        debug!("Error while attempting to open a connection to peer '{}'. {}", &self.target, error.0);
        self.reconnect(ctx); // Begin or continue the reconnect process.
    }
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// NewConnection /////////////////////////////////////////////////////////////////////////////////

/// A message representing that a new connection has been established to the target peer.
#[derive(Message)]
struct NewConnection {
    sink: WsSink,
    stream: WsStream,
}

impl Handler<NewConnection> for WsToPeer {
    type Result = ();

    /// Handler for when a new connection has been successfully open to the target peer.
    fn handle(&mut self, msg: NewConnection, ctx: &mut Self::Context) {
        ctx.add_stream(msg.stream);
        self.connection = ConnectionState::Connected(StateConnected{
            heartbeat: Instant::now(),
            heartbeat_handle: None,
            outbound: SinkWrite::new(msg.sink, ctx),
            handshake: PeerHandshakeState::Initial,
            requests_map: HashMap::new(),
        });

        // Start the connection healthcheck protocol & initialize the peer handshake.
        self.heartbeat(ctx);
        self.handshake(ctx);
    }
}

impl StreamHandler<Frame, WsProtocolError> for WsToPeer {
    /// Handle messages received over the WebSocket.
    fn handle(&mut self, msg: Frame, ctx: &mut Self::Context) {
        use Frame::*;
        match msg {
            Ping(data) => {
                // We've received a ping as part of the heartbeat system. Respond with a pong.
                if let ConnectionState::Connected(state) = &mut self.connection {
                    state.heartbeat = Instant::now();
                }
                self.write_outbound_message(ctx, Message::Pong(data));
            }
            Pong(_) => warn!("Protocol error. Unexpectedly received a pong frame from connected peer."),
            Text(_) => warn!("Protocol error. Unexpectedly received a text frame from connected peer."),
            Binary(None) => warn!("Empty binary payload received from connected peer."),
            Binary(Some(data)) => {
                // Decode the received frame.
                use prost::Message;
                let frame = match api::Frame::decode(data) {
                    Ok(frame) => frame,
                    Err(err) => {
                        error!("Error decoding binary frame from peer connection. {}", err);
                        return;
                    }
                };

                // If the frame is a response frame, route it through to its matching request.
                match frame.payload {
                    Some(api::frame::Payload::Request(req)) => self.route_request(req, frame.meta.unwrap_or_default(), ctx),
                    Some(api::frame::Payload::Response(res)) => self.route_response(res, frame.meta.unwrap_or_default(), ctx),
                    Some(api::frame::Payload::Disconnect(reason)) => {
                        debug!("Received disconnect frame from peer: {}. Closing.", reason);
                        self.connection = ConnectionState::Closing;
                        let _ = match &self.peer_id {
                            Some(nodeid) => self.services.closing_peer_connection.do_send(ClosingPeerConnection(PeerConnectionIdentifier::SocketAddrAndId(self.target, nodeid.clone()))),
                            None => self.services.closing_peer_connection.do_send(ClosingPeerConnection(PeerConnectionIdentifier::SocketAddr(self.target))),
                        };
                        ctx.stop()
                    }
                    None => (),
                }
            }
            Close(reason) => debug!("Connection close message received. {:?}", reason),
        }
    }

    /// Handle errors coming from the connected peer.
    ///
    /// NOTE WELL: we don't take any action here in terms of checking the connection's health. If
    /// the connection is fucked at this point, the healthcheck system will end up triggering a
    /// reconnect event. So here we just log the error.
    fn error(&mut self, err: WsProtocolError, _: &mut Self::Context) -> Running {
        error!("Error received from connected peer. {:?}", err);
        Running::Continue
    }
}

impl WriteHandler<WsProtocolError> for WsToPeer {
    /// Handle errors coming from write attempts on `self.outbound`.
    ///
    /// NOTE WELL: we don't take any action here in terms of checking the connection's health. If
    /// the connection is fucked at this point, the healthcheck system will end up triggering a
    /// reconnect event. So here we just log the error.
    fn error(&mut self, err: WsProtocolError, _: &mut Self::Context) -> Running {
        error!("Error writing outbound message. {:?}", err);
        Running::Continue
    }
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// RetryOutboundMessage //////////////////////////////////////////////////////////////////////////

/// A wrapper type used for retrying attempts to send outbound messages.
#[derive(Message)]
pub(self) struct RetryOutboundMessage(pub Message);

impl Handler<RetryOutboundMessage> for WsToPeer {
    type Result = ();

    /// Handle requests to send outbound messages to the connected peer.
    fn handle(&mut self, msg: RetryOutboundMessage, ctx: &mut Self::Context) {
        self.write_outbound_message(ctx, msg.0);
    }
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// OutboundPeerRequest ///////////////////////////////////////////////////////////////////////////

impl Handler<OutboundPeerRequest> for WsToPeer {
    type Result = ResponseActFuture<Self, api::Response, ()>;

    /// Handle requests to send outbound messages to the connected peer.
    fn handle(&mut self, msg: OutboundPeerRequest, ctx: &mut Self::Context) -> Self::Result {
        Box::new(self.send_outbound_peer_request(msg, ctx))
    }
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// DisconnectPeer ////////////////////////////////////////////////////////////////////////////////

impl Handler<DisconnectPeer> for WsToPeer {
    type Result = ();

    fn handle(&mut self, _: DisconnectPeer, ctx: &mut Self::Context) {
        let frame = api::Frame::new_disconnect(api::Disconnect::ConnectionInvalid, Default::default());
        let data = utils::encode_peer_frame(&frame);
        self.write_outbound_message(ctx, ws::Message::Binary(data.into()));
        ctx.stop();
    }
}
