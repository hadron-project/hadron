//! A module encapsulating the `WsFromPeer` actor and its logic.
//!
//! The `WsFromPeer` actor represents a WebSocket connection which was initialized by a peer
//! cluster member.

use std::{
    time::Instant,
};

use actix::*;
use actix_web_actors::ws;
use log::{info};

use crate::{
    connections::{
        PEER_HB_INTERVAL, PEER_HB_THRESHOLD,
        Connections, PeerHandshakeState,
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
}

impl WsFromPeer {
    /// Create a new instance.
    pub fn new(parent: Addr<Connections>) -> Self {
        Self{parent, heartbeat: Instant::now(), state: PeerHandshakeState::Initial}
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
                // ctx.state().parent.do_send(()); // TODO: send a message to parent actor to clean up this connection.
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
            // We've received a ping from the peer. Update heartbeat on our end, then pong.
            ws::Message::Ping(msg) => {
                self.heartbeat = Instant::now();
                ctx.pong(&msg);
            }

            // We've received a pong from the peer. Update heartbeat on our end.
            ws::Message::Pong(_) => {
                self.heartbeat = Instant::now();
            }

            // We've received a frame from the connected peer.
            ws::Message::Binary(bin) => ctx.binary(bin),

            // All other messages will be dropped with no further action.
            _ => (),
        }
    }
}
