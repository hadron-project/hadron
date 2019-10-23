//! A module encapsulating the `WsFromPeer` actor and its logic.
//!
//! The `WsFromPeer` actor represents a WebSocket connection which was initialized by a peer
//! cluster member.

use std::{
    collections::HashMap,
    time::Instant,
};

use actix::{
    prelude::*,
    fut::FutureResult,
};
use actix_web_actors::ws;
use futures::sync::oneshot;
use log::{debug, error, warn};

use crate::{
    NodeId,
    app::{AppDataResponse, AppDataError, InboundRaftRequest},
    networking::{
        ClientRoutingInfo,
        network::{
            PEER_HB_INTERVAL, PEER_HB_THRESHOLD,
            forwarding::ForwardedRequest,
            peers::{
                ClosingPeerConnection, DisconnectPeer, OutboundPeerRequest,
                PeerAddr, PeerConnectionIdentifier, PeerConnectionLive, PeerHandshakeState,
            },
        },
    },
    proto::peer,
    utils,
};

/// All services needed by the `WsFromPeer` actor.
pub(super) struct WsFromPeerServices {
    pub closing_peer_connection: Recipient<ClosingPeerConnection>,
    pub forwarded_request: Recipient<ForwardedRequest>,
    pub inbound_raft_request: Recipient<InboundRaftRequest>,
    pub peer_connection_live: Recipient<PeerConnectionLive>,
}

impl WsFromPeerServices {
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
// WsFromPeer ////////////////////////////////////////////////////////////////////////////////////

/// An actor responsible for handling inbound WebSocket connections from peers.
///
/// When a socket is received by a peer, once the connection is successfully established, no new
/// connection will be open to the same peer. The one connection will be used for communication
/// both ways until the connection is lost.
///
/// This end of a peer connection will passively follow the handshake protocol. It is the
/// responsibility of the initiator of the connection to drive the handshake protocol.
///
/// The Railgun heartbeat protcol for cluster peer connections is such that only
/// the receiving end of the connection will send pings. That is this end.
pub(super) struct WsFromPeer {
    /// The ID of this node.
    node_id: NodeId,
    /// The services which this actor depends upon.
    services: WsFromPeerServices,
    /// The last successful heartbeat on this socket. Will reckon the peer as being dead after
    /// `PEER_HB_THRESHOLD` has been exceeded since last successful heartbeat.
    heartbeat: Instant,
    /// The handshake state of the connection.
    state: PeerHandshakeState,
    /// The NodeID of the connected peer.
    ///
    /// This will only be available after a successful handshake.
    peer_id: Option<NodeId>,
    /// A map of all pending requests.
    requests_map: HashMap<String, oneshot::Sender<peer::Response>>,
    /// A cached copy of this node's client routing info.
    routing: ClientRoutingInfo,
}

impl WsFromPeer {
    /// Create a new instance.
    pub fn new(services: WsFromPeerServices, node_id: NodeId, routing: ClientRoutingInfo) -> Self {
        Self{
            node_id, services, routing,
            heartbeat: Instant::now(),
            state: PeerHandshakeState::Initial,
            peer_id: None,
            requests_map: Default::default(),
        }
    }

    /// Sever the connection with the peer after sending a disconnect frame.
    fn send_disconnect(&mut self, disconnect: peer::Disconnect, meta: peer::Meta, ctx: &mut ws::WebsocketContext<Self>) {
        let frame = peer::Frame::new_disconnect(disconnect, meta);
        let data = utils::encode_peer_frame(&frame);
        ctx.binary(data);
        if let Some(id) = self.peer_id {
            let _ = self.services.closing_peer_connection.do_send(ClosingPeerConnection(PeerConnectionIdentifier::NodeId(id)));
        }
        ctx.stop();
    }

    /// Handle peer handshake protocol.
    ///
    /// A handshake frame has been received. Update the peer ID based on the given information,
    /// propagate all of this information to the parent `Network` actor, and then response to
    /// the caller with a handshake frame as well.
    fn handshake(&mut self, hs: peer::Handshake, meta: peer::Meta, ctx: &mut ws::WebsocketContext<Self>) {
        // If the connection is being made with self due to initial discovery probe, then respond
        // over the socket with a disconnect frame indicating that such is the case.
        if hs.node_id == self.node_id {
            return self.send_disconnect(peer::Disconnect::ConnectionInvalid, meta, ctx);
        }

        // Update handshake state & peer ID.
        self.state = PeerHandshakeState::Done;
        self.peer_id = Some(hs.node_id);

        // Propagate handshake info to parent `Network` actor.
        let f = self.services.peer_connection_live.send(PeerConnectionLive{
            peer_id: hs.node_id,
            routing: hs.routing,
            addr: PeerAddr::FromPeer(ctx.address()),
        });
        ctx.spawn(fut::wrap_future(f.map_err(|_| ())).map(move |_, act: &mut Self, ctx| act.handshake_response(meta, ctx)));
    }

    /// Handle sending a handshake response.
    fn handshake_response(&mut self, meta: peer::Meta, ctx: &mut ws::WebsocketContext<Self>) {
        // Respond to the caller with a handshake frame.
        // TODO: finish up the routing info pattern. See the peer connection management doc.
        let hs_out = peer::Handshake{node_id: self.node_id, routing: self.routing.clone()};
        let frame = peer::Frame{meta: Some(meta), payload: Some(peer::frame::Payload::Response(peer::Response{
            payload: Some(peer::response::Payload::Handshake(hs_out)),
        }))};
        let data = utils::encode_peer_frame(&frame);
        ctx.binary(data);
    }

    /// Handle forwarded client requests.
    fn handle_forwarded_request(&mut self, msg: peer::ForwardedClientRequest, meta: peer::Meta, ctx: &mut ws::WebsocketContext<Self>) {
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

    /// Setup a heartbeat protocol with the connected peer.
    ///
    /// NOTE WELL: the Railgun heartbeat protcol for cluster peer `Network` is such that only
    /// the receiving end of the connection will send pings.
    fn heartbeat(&self, ctx: &mut ws::WebsocketContext<Self>) {
        ctx.run_interval(PEER_HB_INTERVAL, |act, ctx| {
            // Check client heartbeats.
            if Instant::now().duration_since(act.heartbeat) > PEER_HB_THRESHOLD {
                log::debug!("Peer connection appears to be dead, disconnecting.");
                if let Some(id) = act.peer_id {
                    let _ = act.services.closing_peer_connection.do_send(ClosingPeerConnection(PeerConnectionIdentifier::NodeId(id)));
                }
                ctx.stop();
            } else {
                ctx.ping("");
            }
        });
    }

    /// Route a request over to the parent `Network` actor for handling.
    fn route_request(&mut self, req: peer::Request, meta: peer::Meta, ctx: &mut ws::WebsocketContext<Self>) {
        match req.payload {
            // Only this actor type receives handshake requests.
            Some(peer::request::Payload::Handshake(hs)) => self.handshake(hs, meta, ctx),
            Some(peer::request::Payload::Raft(req)) => {
                // TODO: refactor this into a standalone handler.
                let f = fut::wrap_future(self.services.inbound_raft_request.send(InboundRaftRequest(req, meta.clone())))
                    .map_err(|err, _: &mut Self, _| {
                        error!("Error propagating inbound Raft request. {}", err);
                        peer::Error::Internal
                    })
                    .and_then(|res, _, _| fut::result(res))
                    .then(move |res, act, ctx| act.send_raft_response(res, meta, ctx));
                ctx.spawn(f);
            }
            Some(peer::request::Payload::Routing(routing_info)) => {
                // TODO: impl this.
                error!("Received updated routing info from peer, but handler is not implemented in WsFromPeer. {:?}", routing_info)
            },
            Some(peer::request::Payload::Forwarded(forwarded)) => self.handle_forwarded_request(forwarded, meta, ctx),
            None => log::warn!("Empty request segment received in WsFromPeer."),
        }
    }

    /// Route a response payload received from the socket to its matching request future.
    fn route_response(&mut self, res: peer::Response, meta: peer::Meta, _: &mut ws::WebsocketContext<Self>) {
        // Extract components from request map, send the future's value & cancel its timeout.
        match self.requests_map.remove(&meta.id) {
            None => (), // If there is no handler awaiting the response, drop it.
            Some(tx) => {
                let _ = tx.send(res);
            }
        }
    }

    /// Send a fully structured frame to the connected peer.
    fn send_frame(&mut self, frame: peer::Frame, ctx: &mut ws::WebsocketContext<Self>) -> impl ActorFuture<Actor=Self, Item=(), Error=()> {
        let buf = utils::encode_peer_frame(&frame);
        ctx.binary(buf);
        fut::ok(())
    }

    /// Send the given forwarding response to the connected peer.
    fn send_forwarded_response(&mut self, res: Result<AppDataResponse, AppDataError>, meta: peer::Meta, ctx: &mut ws::WebsocketContext<Self>) -> impl ActorFuture<Actor=Self, Item=(), Error=()> {
        let forwarded_res = match res {
            Ok(data) => peer::forwarded_client_response::Result::Data(utils::bin_encode_app_data_response(&data)),
            Err(err) => peer::forwarded_client_response::Result::Error(utils::bin_encode_app_data_error(&err)),
        };
        let frame = peer::Frame{
            meta: Some(meta),
            payload: Some(peer::frame::Payload::Response(peer::Response{
                payload: Some(peer::response::Payload::Forwarded(peer::ForwardedClientResponse{
                    result: Some(forwarded_res),
                })),
            })),
        };
        self.send_frame(frame, ctx)
    }

    /// Send the given Raft response/error result to the connected peer.
    fn send_raft_response(&mut self, res: Result<peer::RaftResponse, peer::Error>, meta: peer::Meta, ctx: &mut ws::WebsocketContext<Self>) -> impl ActorFuture<Actor=Self, Item=(), Error=()> {
        let frame = peer::Frame{
            meta: Some(meta),
            payload: Some(peer::frame::Payload::Response(peer::Response{
                payload: Some(match res {
                    Ok(raft_res) => peer::response::Payload::Raft(raft_res),
                    Err(err) => peer::response::Payload::Error(err as i32),
                }),
            })),
        };
        self.send_frame(frame, ctx)
    }
}

impl Actor for WsFromPeer {
    type Context = ws::WebsocketContext<Self>;

    /// Logic for starting this actor.
    ///
    /// When the connection is open, a heartbeat protocol is initiated to ensure liveness of the
    /// connection.
    fn started(&mut self, ctx: &mut Self::Context) {
        // Start the heartbeat protocol.
        self.heartbeat(ctx);
    }

    fn stopped(&mut self, _: &mut Self::Context) {
        log::debug!("WsFromPeer connection from {:?} has been stopped.", self.peer_id);
    }

    fn stopping(&mut self, _: &mut Self::Context) -> Running {
        log::debug!("WsFromPeer connection from {:?} is being stopped.", self.peer_id);
        Running::Stop
    }
}

impl StreamHandler<ws::Message, ws::ProtocolError> for WsFromPeer {
    /// Handle messages received over the WebSocket.
    fn handle(&mut self, msg: ws::Message, ctx: &mut Self::Context) {
        match msg {
            ws::Message::Nop => (),
            ws::Message::Close(reason) => debug!("Connection with peer is closing. {:?}", reason),
            ws::Message::Ping(_) => warn!("Protocol error. Unexpectedly received a ping frame from connected peer."),
            ws::Message::Pong(_) => {
                // Heartbeat response received from connected peer.
                self.heartbeat = Instant::now();
            }
            ws::Message::Text(_) => warn!("Protocol error. Unexpectedly received a text frame from connected peer."),
            ws::Message::Binary(data) => {
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
                    Some(peer::frame::Payload::Response(res)) => self.route_response(res, frame.meta.unwrap_or_default(), ctx),
                    Some(peer::frame::Payload::Request(req)) => self.route_request(req, frame.meta.unwrap_or_default(), ctx),
                    Some(peer::frame::Payload::Disconnect(reason)) => {
                        debug!("Received disconnect frame from peer: {}. Closing.", reason);
                        if let Some(id) = self.peer_id {
                            let _ = self.services.closing_peer_connection.do_send(ClosingPeerConnection(PeerConnectionIdentifier::NodeId(id)));
                        }
                        ctx.stop()
                    }
                    None => (),
                }
            }
        }
    }
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// OutboundPeerRequest ///////////////////////////////////////////////////////////////////////////

impl Handler<OutboundPeerRequest> for WsFromPeer {
    type Result = ResponseActFuture<Self, peer::Response, ()>;

    /// Handle requests to send outbound messages to the connected peer.
    ///
    /// NOTE: the error type here will always be `()` because cancellations & timeouts will be
    /// turned into error responses.
    fn handle(&mut self, msg: OutboundPeerRequest, ctx: &mut ws::WebsocketContext<Self>) -> Self::Result {
        // Build the outbound request frame.
        let requestid = uuid::Uuid::new_v4().to_string();
        let frame = peer::Frame{
            meta: Some(peer::Meta{id: requestid.clone()}),
            payload: Some(peer::frame::Payload::Request(msg.request)),
        };

        // Build the response channel and retain the sender.
        let (tx, rx) = oneshot::channel();
        self.requests_map.insert(requestid.clone(), tx);

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
                        let _ = act.requests_map.remove(&requestid); // May still be present on error path. Don't leak.
                        fut::ok(peer::Response::new_error(err))
                    },
                }
            });

        // Serialize the request data and write it over the outbound socket.
        let buf = utils::encode_peer_frame(&frame);
        ctx.binary(buf);

        // Return a future to the caller which will receive the response when it comes back from
        // the peer, else it will timeout.
        Box::new(res)
    }
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// DisconnectPeer ////////////////////////////////////////////////////////////////////////////////

impl Handler<DisconnectPeer> for WsFromPeer {
    type Result = ();

    fn handle(&mut self, _: DisconnectPeer, ctx: &mut Self::Context) {
        let frame = peer::Frame::new_disconnect(peer::Disconnect::ConnectionInvalid, Default::default());
        let data = utils::encode_peer_frame(&frame);
        ctx.binary(data);
        ctx.stop();
    }
}
