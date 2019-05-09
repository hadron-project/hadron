//! A module encapsulating the `WsToPeer` actor and its logic.
//!
//! The `WsToPeer` actor represents a WebSocket connection which was initialized by the source
//! node and sent to a cluster peer.

use std::{
    time::Instant,
};

use actix::*;
use actix_web;
use awc::{
    error::WsProtocolError,
    ws::{Frame, Message},
};
use log::{info};

use crate::{
    connections::{
        PEER_HB_INTERVAL, PEER_HB_THRESHOLD,
        Connections, PeerHandshakeState,
    },
};

//////////////////////////////////////////////////////////////////////////////////////////////////
// WsToPeer ////////////////////////////////////////////////////////////////////////////////////

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
    /// Address of the parent connections actor.
    parent: Addr<Connections>,

    /// The last successful heartbeat on this socket. Will reckon the peer as being dead after
    /// `PEER_HB_THRESHOLD` has been exceeded since last successful heartbeat.
    heartbeat: Instant,

    /// The handshake state of the connection.
    state: PeerHandshakeState,
}

impl WsToPeer {
    /// Create a new instance.
    pub fn new(parent: Addr<Connections>) -> Self {
        Self{parent, heartbeat: Instant::now(), state: PeerHandshakeState::Initial}
    }

    /// Healthcheck the connection on a regular interval.
    ///
    /// In Railgun, the receiving peer in the connection is responsible for sending heartbeats,
    /// so the initiating end just needs to check its last heartbeat on a regular interval to
    /// ensure liveness.
    fn healthcheck(&self, ctx: &mut Context<Self>) {
        ctx.run_interval(PEER_HB_INTERVAL, |act, ctx| {
            // TODO: check liveness.
        });
    }
}

impl Actor for WsToPeer {
    type Context = Context<Self>;

    /// Logic for starting this actor.
    ///
    /// When the connection is open, a heartbeat protocol is initiated to ensure liveness of the
    /// connection.
    fn started(&mut self, ctx: &mut Self::Context) {
        // Start the connection healthcheck protocol.
        self.healthcheck(ctx);

        // TODO: begin the handshake protocol. This will only happen once per connection at the
        // very beginning in order to establish properly structured communications.
    }
}

impl StreamHandler<Frame, WsProtocolError> for WsToPeer {
    /// Handle messages received over the WebSocket.
    fn handle(&mut self, msg: Frame, ctx: &mut Self::Context) {
        // TODO: handle inbound frames.
    }
}
