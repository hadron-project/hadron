//! A module encapsulating the `WsFromPeer` actor and its logic.
//!
//! The `WsFromPeer` actor represents a WebSocket connection which was initialized by a peer
//! cluster member.

use std::{
    time::Instant,
};

use actix::*;
use actix_web_actors::ws;
use log::{debug, info, warn};

use crate::{
    connections::{
        PEER_HB_INTERVAL, PEER_HB_THRESHOLD,
        ClosingPeerConnection, Connections, OutboundMessage,
        PeerConnectionIdentifier, PeerHandshakeState,
    },
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
    peer_node_id: Option<String>,
}

impl WsFromPeer {
    /// Create a new instance.
    pub fn new(parent: Addr<Connections>) -> Self {
        Self{parent, heartbeat: Instant::now(), state: PeerHandshakeState::Initial, peer_node_id: None}
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
                if let Some(id) = act.peer_node_id.as_ref() {
                    act.parent.do_send(ClosingPeerConnection(PeerConnectionIdentifier::NodeId(id.to_string())));
                }
                ctx.stop();
            } else {
                ctx.ping("");
            }
        });
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
            ws::Message::Ping(_) => warn!("Protocol error. Unexpectedly received a ping frame from connected peer."),
            ws::Message::Pong(_) => {
                // Heartbeat received from connected peer.
                self.heartbeat = Instant::now();
            }
            ws::Message::Text(_) => warn!("Protocol error. Unexpectedly received a text frame from connected peer."),
            ws::Message::Binary(bin) => ctx.binary(bin), // TODO: handle inbound frames.
            ws::Message::Close(reason) => {
                debug!("Connection with peer is closing. {:?}", reason);
            }
            ws::Message::Nop => (),
        }
    }
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// OutboundMessage ///////////////////////////////////////////////////////////////////////////////

impl Handler<OutboundMessage> for WsFromPeer {
    type Result = ();

    /// Handle requests to send outbound messages to the connected peer.
    fn handle(&mut self, _msg: OutboundMessage, _ctx: &mut Self::Context) {
        // TODO: implement this.
        // self.write_outbound_message(ctx, msg.0);
    }
}
