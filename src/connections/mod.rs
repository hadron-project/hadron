//! Peer discovery actor abstraction.
//!
//! This module provides an abstraction over the peer discovery system. The interface here is
//! quite simple. All possible discovery actors implement the `Discovery` trait. Based on the
//! runtime configuration for this system, the appropriate discovery actor will be created using
//! this modules `new_discovery_instance` function. The returned object should be used for
//! registering listener to observe peer discovery changes.
//!
//! The discovery actors are stream only actors. They do not expect any input from other actors in
//! this system. Other actors which need to observe the stream of changes coming from this actor
//! should subscribe to this actor.

mod client;
mod from_peer;
mod to_peer;

use std::{
    sync::Arc,
    time::{Duration},
};

use actix::prelude::*;
use actix_web::{
    App, Error, HttpServer, HttpRequest, HttpResponse,
    dev::Server,
    web,
};
use actix_web_actors::ws;
use log::{error, info};

use crate::{
    config::Config,
    connections::{
        from_peer::WsFromPeer,
    },
    discovery::{
        Discovery, ObservedPeersChangeset, SubscribeToDiscoveryChangesets,
    },
};

/// The interval at which heartbeats are sent to peer nodes.
pub(self) const PEER_HB_INTERVAL: Duration = Duration::from_secs(2);

/// The amount of time which is allowed to elapse between successful heartbeats before a
/// connection is reckoned as being dead between peer nodes.
pub(self) const PEER_HB_THRESHOLD: Duration = Duration::from_secs(10);

//////////////////////////////////////////////////////////////////////////////////////////////////
// ServerState ///////////////////////////////////////////////////////////////////////////////////

/// A type used as a shared state context for all WebSocket actor instances.
#[derive(Clone)]
pub(self) struct ServerState {
    pub parent: Addr<Connections>,
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// Connections ///////////////////////////////////////////////////////////////////////////////////

/// An actor responsible for handling all network activity throughout the system.
///
/// See the README.md in this directory for additional information on actor responsibilities.
pub struct Connections {
    discovery: Addr<Discovery>,
    config: Arc<Config>,
    has_successful_start: bool,
    server: Option<Server>,
}

impl Connections {
    /// Create a new instance.
    pub fn new(discovery: Addr<Discovery>, config: Arc<Config>) -> Self {
        let has_successful_start = false;
        Self{discovery, config, has_successful_start, server: None}
    }

    /// Build a new network server instance for use by this system.
    pub fn build_server(&self, ctx: &Context<Self>) -> Result<Server, ()> {
        let data = ServerState{parent: ctx.address()};
        let server = HttpServer::new(move || {
            App::new().data(data.clone())
                // This endpoint is used for internal client communication.
                .service(web::resource("/internal/").to(Self::handle_peer_connection))
                // .service(web::resource("").to(Self::handle_client_connection))
                // TODO: setup a catchall handler.
        })
        .bind(format!("0.0.0.0:{}", &self.config.port))
        .map_err(|err| {
            error!("Error building network stack. System will shutdown. {}", err);
            actix::System::current().stop();
        })?
        .start();

        Ok(server)
    }

    /// Handler for opening new peer WebSocket connections.
    fn handle_peer_connection(req: HttpRequest, stream: web::Payload, data: web::Data<ServerState>) -> Result<HttpResponse, Error> {
        ws::start(WsFromPeer::new(data.parent.clone()), &req, stream)
    }
}

impl Actor for Connections {
    type Context = Context<Self>;

    /// Logic for starting this actor.
    fn started(&mut self, ctx: &mut Self::Context) {
        // Build the network server.
        if let Ok(addr) = self.build_server(&ctx) {
            self.server = Some(addr);
            self.has_successful_start = true;
        }

        // Subscribe to the discovery system's changesets. This happens only once when the system
        // is booted. This should never fail.
        self.discovery.do_send(SubscribeToDiscoveryChangesets(ctx.address().recipient()));
    }
}

impl Handler<ObservedPeersChangeset> for Connections {
    type Result = ();

    /// Handle changesets coming from the discovery system.
    ///
    /// This routine is responsible for checking each of the entries in the changeset to drive
    /// logic for connecting to new peers, or flagging a peer as missing. Flagging a missing peer
    /// is important as the system will attempt to reconnect to peers any time a connection is
    /// lost. If the peer has disappeared from the discovery system, then the reconnect system
    /// will stop attempting to reconnect to the peer.
    ///
    /// When new changesets come in from the discovery system, this handler should check the
    /// routing table to ensure that a connection to the target IP doesn't already exist. It
    /// would be rare for this to happen, but it is always possible for a peer to be removed from
    /// the discovery system by the end-user (for maintenence or the like) without every actually
    /// taking the node down. In such a case, the peer will still have a live connection even
    /// though the discovery system reckoned it as being missing. In such a case, a new connection
    /// will not be made.
    ///
    /// Railgun does not attempt to filter out its own addresses from the discovery protocol.
    /// Typically the discovery protocol will yeild an entry which refers to a nodes own IP. In
    /// such a case, the connection will be established, but the handshake protocol will determine
    /// that it is a connection to self, and will drop the connection.
    ///
    /// The peer connection handshake protocol will also consult the routing table to check if a
    /// peer with the same Node ID already has an open connection. A few reasons why this might
    /// happen:
    ///
    /// - The end-users discovery system has added a new IP which points to a node which already
    ///   has an IP in the list. Perhaps this is for a migration or the like.
    /// - A connected peer has died but was immediately brought back bearing the same Node ID but
    ///   on a different IP. The original connection may still be in a retry state and the
    ///   discovery system may not have observed the removal yet.
    ///
    /// When situations like this arise, it is critical that the new IP pointing to the same node
    /// not be discarded. It needs to be preserved in case of a disconnect, where only the new IP
    /// could be successfully connected to, but where the discovery system may not yield the new
    /// IP again.
    ///
    /// **The approach we take** is to maintain the registry of all observed peers in the routing
    /// table. When the peer connection handshake determines that the new connection is to a peer
    /// with which a connection already exists, then the new connection will be dropped, but the
    /// new IP will be registered as a failover IP for the current connection.
    ///
    /// The peer handshake protocol ensures that duplicate connections will not be finalized. In
    /// scenarios where a reconnect is attempted and fails and a failover IP needs to be used, the
    /// reconnect protocol will ensure the old connection is removed from the routing table to
    /// ensure a consistent and lean connection set.
    ///
    /// See docs/internals/peer-connection-management.md for more details.
    fn handle(&mut self, changeset: ObservedPeersChangeset, _: &mut Self::Context) -> Self::Result {
        // TODO: build connnections.
        info!("Received changeset for connections: {:?}", changeset);
    }
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// PeerHandshakeState ////////////////////////////////////////////////////////////////////////////

/// The peer handshake protocol state.
///
/// The handshake protocol is a very simple protocol driven by the node responsible for
/// initializing the connection. The handshake protocol beings after the base WebSocket connection
/// has been successfully established. If the handshake protcol fails, the connection will be
/// droped.
///
/// ### Initial
/// - Send node ID, node state, routing info, and discovered peers to receiver.
/// - Receiver checks to ensure it doesn't already have a connection with the sender.
///     - If a connection already exists with sender, a disconnect frame will be sent to sender,
///       and the connection will be closed.
///     - Else, the receiver will update its internal state with the received data and then send
///       the equivalent payload over to the sender as a response.
/// - Sender receives equivalent payload, and performs same check with same conditions.
///
/// ### Confirmation
/// - If everything is still in order, a confirmation frame is sent to receiver.
/// - Reciever will attempt to register the connection as live with its `Connections` actor.
///     - If the operation fails because another connection was already open with the same node,
///       then a disconnect frame will be sent back to the sender and the connection will be
///       dropped. Sender will drop connection upon disconnect frame receipt.
///     - If the connection is successfully registered, then the equivalent confirmation frame is
///       sent back to the sender.
/// - Sender receives confirmation frame from receiver and performs same operations.
///
/// Once the above steps have been finished, the handshake will be complete and the connection
/// will be available for general use.
pub(self) enum PeerHandshakeState {
    /// The initial phase of the handshake protocol.
    Initial,
    /// The confirmation phase of the handshake protocol.
    Confirmation,
    /// The finished state of the handshake protocol.
    Done,
}
