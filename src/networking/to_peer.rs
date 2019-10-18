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
    fut::FutureResult,
};
use actix_web::client::Client;
use backoff::{backoff::Backoff, ExponentialBackoff};
use futures::{
    prelude::*,
    sync::{mpsc, oneshot},
};
use awc::{
    error::{WsProtocolError},
    http::StatusCode,
    ws::{Frame, Message as WsMessage},
};
use log::{debug, error, warn};
use prost;

use crate::{
    NodeId,
    app::{AppDataResponse, AppDataError, InboundRaftRequest},
    networking::network::{
        PEER_HB_INTERVAL, PEER_HB_THRESHOLD, PEER_HANDSHAKE_TIMEOUT,
        forwarding::ForwardedRequest,
        peers::{
            ClosingPeerConnection, DisconnectPeer, OutboundPeerRequest,
            PeerAddr, PeerConnectionIdentifier, PeerConnectionLive, PeerHandshakeState,
        },
    },
    proto::peer,
    utils,
};

//////////////////////////////////////////////////////////////////////////////////////////////////
// WsToPeerServices //////////////////////////////////////////////////////////////////////////////

/// All services needed by the `WsToPeer` actor.
pub(super) struct WsToPeerServices {
    pub closing_peer_connection: Recipient<ClosingPeerConnection>,
    pub forwarded_request: Recipient<ForwardedRequest>,
    pub inbound_raft_request: Recipient<InboundRaftRequest>,
    pub peer_connection_live: Recipient<PeerConnectionLive>,
}

impl WsToPeerServices {
    /// Create a new instance.
    pub fn new(
        closing_peer_connection: Recipient<ClosingPeerConnection>,
        forwarded_request: Recipient<ForwardedRequest>,
        inbound_raft_request: Recipient<InboundRaftRequest>,
        peer_connection_live: Recipient<PeerConnectionLive>,
    ) -> Self {
        Self{closing_peer_connection, forwarded_request, inbound_raft_request, peer_connection_live}
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
    /// The channel used to send outbound frames.
    outbound: mpsc::UnboundedSender<WsMessage>,
    /// The handshake state of the connection.
    handshake: PeerHandshakeState,
    /// A map of all pending requests.
    requests_map: HashMap<String, oneshot::Sender<peer::Response>>,
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
    /// A cached copy of this node's client routing info.
    routing: peer::RoutingInfo,
}

impl WsToPeer {
    /// Create a new instance.
    pub fn new(services: WsToPeerServices, node_id: NodeId, target: SocketAddr, routing: peer::RoutingInfo) -> Self {
        Self{
            node_id, services, target, routing,
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
            Initial => peer::Request{segment: Some(peer::request::Segment::Handshake(
                peer::Handshake{node_id: self.node_id, routing: Some(self.routing.clone())}
            ))},
            Done => return,
        };

        // Spawn the outbound request.
        let request = OutboundPeerRequest{request, target_node: 0, timeout: PEER_HANDSHAKE_TIMEOUT};
        let f = self.handle_outbound_peer_request(request, ctx)
            .map(|res, act, ctx| act.handshake_response(res, ctx))
            .map_err(|_, act, ctx| act.handshake(ctx));
        ctx.spawn(f);
    }

    /// Handle a handshake response.
    fn handshake_response(&mut self, res: peer::Response, ctx: &mut Context<Self>) {
        // Get a handle to the current handshake state.
        let state = match &mut self.connection {
            ConnectionState::Connected(state) => state,
            _ => return,
        };

        // Extract handshake response segment, else handle errors.
        let hs = match res.segment {
            Some(peer::response::Segment::Handshake(hs)) => hs,
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
        let f = self.services.peer_connection_live.send(PeerConnectionLive{
            peer_id: hs.node_id,
            routing: hs.routing.unwrap_or_default(),
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

    /// Handle forwarded client requests.
    fn handle_forwarded_request(&mut self, msg: peer::ForwardedClientRequest, meta: peer::Meta, ctx: &mut Context<Self>) {
        log::debug!("Handling forwarded request.");
        let payload = match utils::bin_decode_client_payload(msg.payload) {
            Ok(payload) => payload,
            Err(err) => {
                let f = self.send_forwarded_response(Err(err), meta, ctx);
                ctx.spawn(f);
                return;
            }
        };
        ctx.spawn(fut::wrap_future(self.services.forwarded_request.send(ForwardedRequest::new(payload))
            .map_err(|err| {
                log::error!("Error while handling forwarding request. {}", err);
                AppDataError::Internal
            })
            .and_then(|res| res))
            .then(move |res, act: &mut Self, ctx| act.send_forwarded_response(res, meta, ctx)));
    }

    /// Attempt to build a new connection to the target peer.
    fn new_connection(&mut self, ctx: &mut Context<Self>) {
        ctx.spawn(fut::wrap_future(Client::new().ws(format!("ws://{}/internal/", &self.target)).connect())
            .map_err(|err, act: &mut Self, ctx| {
                log::error!("Error while attempting to connect to cluster. {:?}", err);
                act.reconnect(ctx);
            })
            .and_then(|res, act, ctx| {
                let (res, framed) = res;
                if res.status() != StatusCode::SWITCHING_PROTOCOLS {
                    log::error!("Received unexpected status code '{}' from peer connection request, expected code 101.", res.status());
                    act.reconnect(ctx);
                    return fut::err(());
                }

                // Connect inbound message stream.
                let (sink, stream) = framed.split();
                ctx.add_stream(stream);

                // Create a channel->stream->sink pipeline for sending messages to the cluster, and spawn it.
                let (outbound, outrx) = mpsc::unbounded::<WsMessage>();
                let outrx = outrx.map_err(|_| {
                    log::error!("Error while processing outbound frame, which should never happen.");
                    WsProtocolError::Io(std::io::Error::from(std::io::ErrorKind::Other))
                });
                ctx.spawn(fut::wrap_future(sink.send_all(outrx)
                    .map_err(|err| log::error!("Error sending frame to connected peer. {}", err))
                    // This will clean up the sink's memory, and will happen when the
                    // sender is dropped & everything has been flushed.
                    .map(|_| ())));

                act.connection = ConnectionState::Connected(StateConnected{
                    heartbeat: Instant::now(),
                    heartbeat_handle: None,
                    outbound,
                    handshake: PeerHandshakeState::Initial,
                    requests_map: Default::default(),
                });

                // Start the connection healthcheck protocol & initialize the peer handshake.
                act.heartbeat(ctx);
                act.handshake(ctx);

                fut::ok(())
            }));
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
    fn route_request(&mut self, req: peer::Request, meta: peer::Meta, ctx: &mut Context<Self>) {
        match req.segment {
            // Only this actor type sends handshake requests, so log an error if one is observed here.
            Some(peer::request::Segment::Handshake(_)) => warn!("Handshake request received by a WsToPeer actor. This is a protocol violation."),
            Some(peer::request::Segment::Raft(req)) => {
                let f = fut::wrap_future(self.services.inbound_raft_request.send(InboundRaftRequest(req, meta.clone())))
                    .map_err(|err, _: &mut Self, _| {
                        error!("Error propagating inbound Raft request. {}", err);
                        peer::Error::Internal
                    })
                    .and_then(|res, _, _| fut::result(res))
                    .then(move |res, act, ctx| act.send_raft_response(res, meta, ctx));
                ctx.spawn(f);
            }
            Some(peer::request::Segment::Routing(routing_info)) => {
                // TODO: impl this.
                error!("Received updated routing info from peer, but handler is not implemented in WsToPeer. {:?}", routing_info)
            },
            Some(peer::request::Segment::Forwarded(forwarded)) => self.handle_forwarded_request(forwarded, meta, ctx),
            None => warn!("Empty request segment received in WsToPeer."),
        }
    }

    /// Route a response payload received from the socket to its matching request future.
    fn route_response(&mut self, res: peer::Response, meta: peer::Meta, _: &mut Context<Self>) {
        let state = match &mut self.connection {
            ConnectionState::Connected(state) => state,
            _ => return,
        };

        // Extract components from request map, send the future's value & cancel its timeout.
        match state.requests_map.remove(&meta.id) {
            None => (), // If there is no handler awaiting the response, drop it.
            Some(tx) => {
                let _ = tx.send(res);
            }
        }
    }

    /// Send a fully structured frame to the connected peer.
    fn send_frame(&mut self, frame: peer::Frame, ctx: &mut Context<Self>) -> impl ActorFuture<Actor=Self, Item=(), Error=()> {
        let buf = utils::encode_peer_frame(&frame);
        self.write_outbound_message(ctx, WsMessage::Binary(buf.into()));
        fut::ok(())
    }

    /// Send the given forwarding response to the connected peer.
    fn send_forwarded_response(&mut self, res: Result<AppDataResponse, AppDataError>, meta: peer::Meta, ctx: &mut Context<Self>) -> impl ActorFuture<Actor=Self, Item=(), Error=()> {
        let forwarded_res = match res {
            Ok(data) => peer::forwarded_client_response::Result::Data(utils::bin_encode_app_data_response(&data)),
            Err(err) => peer::forwarded_client_response::Result::Error(utils::bin_encode_app_data_error(&err)),
        };
        let frame = peer::Frame{
            meta: Some(meta),
            payload: Some(peer::frame::Payload::Response(peer::Response{
                segment: Some(peer::response::Segment::Forwarded(peer::ForwardedClientResponse{
                    result: Some(forwarded_res),
                })),
            })),
        };
        self.send_frame(frame, ctx)
    }

    /// Send the given Raft response/error result to the connected peer.
    fn send_raft_response(&mut self, res: Result<peer::RaftResponse, peer::Error>, meta: peer::Meta, ctx: &mut Context<Self>) -> impl ActorFuture<Actor=Self, Item=(), Error=()> {
        let frame = peer::Frame{
            meta: Some(meta),
            payload: Some(peer::frame::Payload::Response(peer::Response{
                segment: Some(match res {
                    Ok(raft_res) => peer::response::Segment::Raft(raft_res),
                    Err(err) => peer::response::Segment::Error(err as i32),
                }),
            })),
        };
        self.send_frame(frame, ctx)
    }

    /// Write an outbound message to the connected peer.
    fn write_outbound_message(&mut self, _: &mut Context<Self>, msg: WsMessage) {
        match &mut self.connection {
            ConnectionState::Connected(state) => {
                let _ = state.outbound.unbounded_send(msg);
            }
            _ => (),
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

    fn stopped(&mut self, _: &mut Self::Context) {
        log::debug!("WsToPeer connection to {:?} has been stopped.", self.peer_id);
    }

    fn stopping(&mut self, _: &mut Self::Context) -> Running {
        if let ConnectionState::Closing = self.connection {
            log::debug!("WsToPeer connection to {:?} is in Closing state & has hit shutdown routine. Stopping.", self.peer_id);
            Running::Stop
        } else {
            log::debug!("WsToPeer connection to {:?} is not in Closing state. Continuing.", self.peer_id);
            Running::Continue
        }
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
                self.write_outbound_message(ctx, WsMessage::Pong(data));
            }
            Pong(_) => warn!("Protocol error. Unexpectedly received a pong frame from connected peer."),
            Text(_) => warn!("Protocol error. Unexpectedly received a text frame from connected peer."),
            Binary(None) => warn!("Empty binary payload received from connected peer."),
            Binary(Some(data)) => {
                // Decode the received frame.
                use prost::Message;
                let frame = match peer::Frame::decode(data) {
                    Ok(frame) => frame,
                    Err(err) => {
                        error!("Error decoding binary frame from peer connection. {}", err);
                        return;
                    }
                };

                // If the frame is a response frame, route it through to its matching request.
                match frame.payload {
                    Some(peer::frame::Payload::Request(req)) => self.route_request(req, frame.meta.unwrap_or_default(), ctx),
                    Some(peer::frame::Payload::Response(res)) => self.route_response(res, frame.meta.unwrap_or_default(), ctx),
                    Some(peer::frame::Payload::Disconnect(reason)) => {
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
// OutboundPeerRequest ///////////////////////////////////////////////////////////////////////////

impl Handler<OutboundPeerRequest> for WsToPeer {
    type Result = ResponseActFuture<Self, peer::Response, ()>;

    /// Handle requests to send outbound messages to the connected peer.
    ///
    /// NOTE: the error type here will always be `()` because cancellations & timeouts will be
    /// turned into error responses.
    fn handle(&mut self, msg: OutboundPeerRequest, ctx: &mut Self::Context) -> Self::Result {
        Box::new(self.handle_outbound_peer_request(msg, ctx))
    }
}

impl WsToPeer {
    /// Send the given outbound peer request and await a response.
    ///
    /// NOTE: the error type here will always be `()` because cancellations & timeouts will be
    /// turned into error responses.
    fn handle_outbound_peer_request(&mut self, msg: OutboundPeerRequest, ctx: &mut Context<Self>) -> impl ActorFuture<Actor=Self, Item=peer::Response, Error=()> {
        let state = match &mut self.connection {
            ConnectionState::Connected(state) => state,
            _ => return fut::Either::A(fut::err(())), // If the peer is not connected, the request will timeout. Drop.
        };

        // Build the outbound request frame.
        let requestid = uuid::Uuid::new_v4().to_string();
        let frame = peer::Frame{
            meta: Some(peer::Meta{id: requestid.clone()}),
            payload: Some(peer::frame::Payload::Request(msg.request)),
        };

        // Build the response channel and retain the sender.
        let (tx, rx) = oneshot::channel();
        state.requests_map.insert(requestid.clone(), tx);

        // Build the response chain.
        let res = fut::wrap_future(rx
            .map_err(|cancelled_err| {
                log::error!("Outbound peer request was unexpectedly cancelled. {}", cancelled_err);
                peer::Error::Internal
            }))
            .timeout(msg.timeout, peer::Error::Timeout)
            .then(move |res, act: &mut Self, _| -> FutureResult<peer::Response, (), Self> {
                match res {
                    Ok(val) => fut::ok(val),
                    Err(err) => {
                        let state = match &mut act.connection {
                            ConnectionState::Connected(state) => state,
                            _ => return fut::ok(peer::Response::new_error(err)),
                        };
                        let _ = state.requests_map.remove(&requestid); // May still be present on error path. Don't leak.
                        fut::ok(peer::Response::new_error(err))
                    },
                }
            });

        // Serialize the request data and write it over the outbound socket.
        let buf = utils::encode_peer_frame(&frame);
        self.write_outbound_message(ctx, WsMessage::Binary(buf.into()));

        // Return a future to the caller which will receive the response when it comes back from
        // the peer, else it will timeout.
        fut::Either::B(res)
    }
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// DisconnectPeer ////////////////////////////////////////////////////////////////////////////////

impl Handler<DisconnectPeer> for WsToPeer {
    type Result = ();

    fn handle(&mut self, _: DisconnectPeer, ctx: &mut Self::Context) {
        let frame = peer::Frame::new_disconnect(peer::Disconnect::ConnectionInvalid, Default::default());
        let data = utils::encode_peer_frame(&frame);
        self.write_outbound_message(ctx, WsMessage::Binary(data.into()));
        ctx.stop();
    }
}
