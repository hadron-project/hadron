//! A module encapsulating the `WsFromPeer` actor and its logic.
//!
//! The `WsFromPeer` actor represents a WebSocket connection which was initialized by a peer
//! cluster member.

use std::{
    collections::HashMap,
    time::Instant,
};

use actix::*;
use actix_web_actors::ws;
use futures::{
    prelude::*,
    sync::oneshot,
};
use log::{debug, error, info, warn};

use crate::{
    connections::{
        PEER_HB_INTERVAL, PEER_HB_THRESHOLD,
        ClosingPeerConnection, Connections, OutboundPeerRequest,
        PeerAddr, PeerConnectionIdentifier, PeerConnectionLive, PeerHandshakeState,
    },
    proto::{peer},
};

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
    node_id: String,

    /// Address of the parent connections actor.
    parent: Addr<Connections>,

    /// The last successful heartbeat on this socket. Will reckon the peer as being dead after
    /// `PEER_HB_THRESHOLD` has been exceeded since last successful heartbeat.
    heartbeat: Instant,

    /// The handshake state of the connection.
    state: PeerHandshakeState,

    /// The NodeID of the connected peer.
    ///
    /// This will only be available after a successful handshake.
    peer_id: Option<String>,

    /// A map of all pending requests.
    requests_map: HashMap<String, (oneshot::Sender<Result<peer::api::Response, ()>>, SpawnHandle)>,
}

impl WsFromPeer {
    /// Create a new instance.
    pub fn new(parent: Addr<Connections>, node_id: String) -> Self {
        Self{
            node_id,
            parent,
            heartbeat: Instant::now(),
            state: PeerHandshakeState::Initial,
            peer_id: None,
            requests_map: HashMap::new(),
        }
    }

    /// Sever the connection with the peer after sending a disconnect frame.
    fn disconnect(&mut self, disconnect: peer::api::Disconnect, ctx: &mut ws::WebsocketContext<Self>) {
        debug!("Peer connection needs disconnect: {}. Closing.", disconnect as i32);
        use prost::Message;
        let frame = peer::api::Frame{meta: None, payload: Some(peer::api::frame::Payload::Disconnect(disconnect as i32))};
        let mut data = bytes::BytesMut::with_capacity(frame.encoded_len());
        let _ = frame.encode(&mut data).map_err(|err| error!("Failed to serialize protobuf frame. {}", err));
        ctx.binary(data);
        ctx.stop();
    }

    /// Handle peer handshake protocol.
    ///
    /// A handshake frame has been received. Update the peer ID based on the given information,
    /// propagate all of this information to the parent connections actor, and then response to
    /// the caller with a handshake frame as well.
    fn handshake(&mut self, hs: peer::handshake::Handshake, meta: peer::api::Meta, ctx: &mut ws::WebsocketContext<Self>) {
        // If the connection is being made with self due to initial discovery probe, then respond
        // over the socket with a disconnect frame indicating that such is the case.
        if hs.node_id == self.node_id {
            use prost::Message;
            let frame = peer::api::Frame::new_disconnect(peer::api::Disconnect::ConnectionInvalid, meta);
            let mut buf = bytes::BytesMut::with_capacity(frame.encoded_len());
            let _ = frame.encode(&mut buf).map_err(|err| error!("Failed to encode protobuf frame. {}", err));
            ctx.binary(buf);
            return ctx.stop();
        }

        // Update handshake state & peer ID.
        self.state = PeerHandshakeState::Done;
        self.peer_id = Some(hs.node_id.clone());

        // Propagate handshake info to parent connections actor. If disconnect is needed, send
        // disconnect frame over to peer so that it will not attempt to reconnect.
        let f = self.parent.send(PeerConnectionLive{peer_id: hs.node_id, routing_info: hs.routing_info, addr: PeerAddr::FromPeer(ctx.address())});
        let af = actix::fut::wrap_future(f)
            .map_err(|_, _, _| ()) // NOTE: would only get hit on a timeout or closed. Neither will be hit.
            .and_then(|res, iself: &mut Self, ictx: &mut ws::WebsocketContext<Self>| match res {
                Ok(()) => actix::fut::ok(()),
                Err(disconnect) => {
                    iself.disconnect(disconnect, ictx);
                    actix::fut::err(())
                }
            })
            .map(move |_, iself, ictx| iself.handshake_response(meta, ictx));
        ctx.spawn(af);
    }

    /// Handle sending a handshake response.
    fn handshake_response(&mut self, meta: peer::api::Meta, ctx: &mut ws::WebsocketContext<Self>) {
        // Respond to the caller with a handshake frame.
        // TODO: finish up the routing info pattern. See the peer connection management doc.
        use prost::Message;
        let hs_out = peer::handshake::Handshake{node_id: self.node_id.clone(), routing_info: String::with_capacity(0)};
        let frame = peer::api::Frame{meta: Some(meta), payload: Some(peer::api::frame::Payload::Response(peer::api::Response{
            segment: Some(peer::api::response::Segment::Handshake(hs_out)),
        }))};
        let mut data = bytes::BytesMut::with_capacity(frame.encoded_len());
        let _ = frame.encode(&mut data).map_err(|err| error!("Failed to serialize protobuf frame. {}", err));
        ctx.binary(data);
    }

    /// Setup a heartbeat protocol with the connected peer.
    ///
    /// NOTE WELL: the Railgun heartbeat protcol for cluster peer connections is such that only
    /// the receiving end of the connection will send pings.
    fn heartbeat(&self, ctx: &mut ws::WebsocketContext<Self>) {
        ctx.run_interval(PEER_HB_INTERVAL, |act, ctx| {
            // Check client heartbeats.
            if Instant::now().duration_since(act.heartbeat) > PEER_HB_THRESHOLD {
                info!("Peer connection appears to be dead, disconnecting.");
                if let Some(id) = act.peer_id.as_ref() {
                    act.parent.do_send(ClosingPeerConnection(PeerConnectionIdentifier::NodeId(id.to_string())));
                }
                ctx.stop();
            } else {
                ctx.ping("");
            }
        });
    }

    /// Route a request over to the parent connections actor for handling.
    fn route_request(&mut self, req: peer::api::Request, meta: peer::api::Meta, ctx: &mut ws::WebsocketContext<Self>) {
        // Handshakes will only ever be initiated by a `WsToPeer` actor (not this one), so we need
        // to check for and handle handshake requests here.
        match req.segment {
            Some(peer::api::request::Segment::Handshake(hs)) => {
                self.handshake(hs, meta, ctx)
            }
            None => {
                // NOTE: this will pretty much never be hit.
                warn!("Empty request segment received in WsFromPeer.");
            }
            // _ => self.parent.do_send(InboundPeerRequest(req, meta)), // NOTE: compiler will force this to be re-enabled once there are more variants.
        }
    }

    /// Route a response payload received from the socket to its matching request future.
    fn route_response(&mut self, res: peer::api::Response, meta: peer::api::Meta, ctx: &mut ws::WebsocketContext<Self>) {
        // Extract components from request map, send the future's value & cancel its timeout.
        match self.requests_map.remove(&meta.id) {
            None => (),
            Some((tx, timeouthandle)) => {
                let _ = tx.send(Ok(res));
                ctx.cancel_future(timeouthandle);
            }
        }
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
                debug!("Binary data received.");
                use prost::Message;
                let frame = match peer::api::Frame::decode(data) {
                    Ok(frame) => frame,
                    Err(err) => {
                        error!("Error decoding binary frame from peer connection. {}", err);
                        return;
                    }
                };

                // If the frame is a response frame, route it through to its matching request.
                match frame.payload {
                    Some(peer::api::frame::Payload::Response(res)) => self.route_response(res, frame.meta.unwrap_or_default(), ctx),
                    Some(peer::api::frame::Payload::Request(req)) => self.route_request(req, frame.meta.unwrap_or_default(), ctx),
                    Some(peer::api::frame::Payload::Disconnect(reason)) => {
                        debug!("Received peer disconnect frame {}. Closing.", reason);
                        if let Some(id) = self.peer_id.as_ref() {
                            self.parent.do_send(ClosingPeerConnection(PeerConnectionIdentifier::NodeId(id.to_string())));
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
    type Result = ResponseFuture<peer::api::Response, ()>;

    /// Handle requests to send outbound messages to the connected peer.
    fn handle(&mut self, msg: OutboundPeerRequest, ctx: &mut ws::WebsocketContext<Self>) -> Self::Result {
        // Build the outbound request frame.
        let requestid = uuid::Uuid::new_v4().to_string();
        let deadline = (chrono::Utc::now() + chrono::Duration::seconds(msg.timeout.as_secs() as i64)).timestamp_millis();
        let frame = peer::api::Frame{
            meta: Some(peer::api::Meta{id: requestid.clone(), deadline}),
            payload: Some(peer::api::frame::Payload::Request(msg.request)),
        };

        // Spawn the request's timeout handler & retain the spawnhandle.
        let closed_requestid = requestid.clone();
        let timeout = ctx.run_later(msg.timeout, move |closed_self, _closed_ctx| {
            if let Some((_, _)) = closed_self.requests_map.remove(&closed_requestid) {
                debug!("Request '{}' timedout.", &closed_requestid);
            }
        });

        // Create the request/response channel & add components to request map.
        let (tx, rx) = oneshot::channel();
        self.requests_map.insert(requestid, (tx, timeout));

        // Serialize the request data and write it over the outbound socket.
        use prost::Message;
        let mut buf = bytes::BytesMut::with_capacity(frame.encoded_len());
        let _ = frame.encode(&mut buf).map_err(|err| {
            error!("Failed to serialize protobuf frame. {}", err);
        });
        ctx.binary(buf);

        // Return a future to the caller which will receive the response when it comes back from
        // the peer, else it will timeout.
        Box::new(rx
            .map_err(|err| error!("Error from OutboundPeerRequest receiver. {}", err))
            .and_then(|res| match res {
                Ok(response) => Ok(response),
                Err(_) => Err(()),
            })
        )
    }
}
