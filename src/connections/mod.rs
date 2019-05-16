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
    collections::HashMap,
    net::SocketAddr,
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
use awc::ws::Message;
use log::{debug, error};
use uuid::Uuid;

use crate::{
    config::Config,
    connections::{
        from_peer::WsFromPeer,
        to_peer::{DiscoveryState, UpdateDiscoveryState, WsToPeer},
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
    pub node_id: String,
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// WsToPeerState /////////////////////////////////////////////////////////////////////////////////

/// A type used to track the discovery state of a `WsToPeer`'s socket.
///
/// This helps to cut down on superfluous messaging.
struct WsToPeerState {
    addr: Addr<WsToPeer>,
    discovery_state: DiscoveryState,
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// Connections ///////////////////////////////////////////////////////////////////////////////////

/// An actor responsible for handling all network activity throughout the system.
///
/// See the README.md in this directory for additional information on actor responsibilities.
pub struct Connections {
    node_id: String,
    discovery: Addr<Discovery>,
    config: Arc<Config>,
    server: Option<Server>,
    socketaddr_to_peer: HashMap<SocketAddr, WsToPeerState>,
}

impl Connections {
    /// Create a new instance.
    pub fn new(discovery: Addr<Discovery>, config: Arc<Config>) -> Self {
        Self{
            node_id: Uuid::new_v4().to_string(), // TODO: this should come from startup, from the DB layer.
            discovery,
            config,
            server: None,
            socketaddr_to_peer: HashMap::new(),
        }
    }

    /// Build a new network server instance for use by this system.
    pub fn build_server(&self, ctx: &Context<Self>) -> Result<Server, ()> {
        let data = ServerState{parent: ctx.address(), node_id: self.node_id.clone()};
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
        debug!("Handling a new peer connection request.");
        ws::start(WsFromPeer::new(data.parent.clone(), data.node_id.clone()), &req, stream)
    }
}

impl Actor for Connections {
    type Context = Context<Self>;

    /// Logic for starting this actor.
    fn started(&mut self, ctx: &mut Self::Context) {
        // Build the network server.
        match self.build_server(&ctx) {
            Ok(addr) => self.server = Some(addr),
            Err(_) => {
                // In an errored case, the system will have already been instructed to stop,
                // so here we just stop this actor and return.
                ctx.stop();
                return
            }
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
    /// See docs/internals/peer-connection-management.md for more details.
    ///
    /// Our main objectives here are:
    ///
    /// - Connect to new addrs if we don't already have connections to them.
    /// - Update the discovery state of connections. If they've disappeared from the discovery
    ///   system, then update its discovery state to `Disappeared`. If it had previously
    ///   disappeared, then update its discovery state to `Observed`.
    fn handle(&mut self, changeset: ObservedPeersChangeset, ctx: &mut Self::Context) -> Self::Result {
        // Determine which new addrs need to be spawned. Update discovery states & filter.
        let tospawn: Vec<_> = changeset.new_peers.into_iter()
            .filter(|socketaddr| match self.socketaddr_to_peer.get_mut(socketaddr) {
                // If a disappeared addr has come back, then update it, notify actor & filter.
                Some(ref mut state) if state.discovery_state == DiscoveryState::Disappeared => {
                    debug!("Updating discovery state of connection '{}' to 'Observed'.", &socketaddr);
                    state.discovery_state = DiscoveryState::Observed;
                    state.addr.do_send(UpdateDiscoveryState(DiscoveryState::Observed));
                    false
                }
                // Socket state is good, filter this element out.
                Some(_) => false,
                // New socket addrs will not be filtered out.
                None => true,
            }).collect();

        // Spawn new outbound connections to peers.
        tospawn.into_iter().for_each(|socketaddr| {
            debug!("Spawning a new outbound peer connection to '{}'.", &socketaddr);
            let addr = WsToPeer::new(ctx.address(), self.node_id.clone(), socketaddr).start();
            let discovery_state = DiscoveryState::Observed;
            self.socketaddr_to_peer.insert(socketaddr, WsToPeerState{addr, discovery_state});
        });

        // For any addr which has disappeared from the discovery system, update it here if needed.
        changeset.purged_peers.into_iter()
            .for_each(|socketaddr| match self.socketaddr_to_peer.get_mut(&socketaddr) {
                None => (),
                Some(state) => {
                    debug!("Updating discovery state of connection '{}' to 'Disappeared'.", &socketaddr);
                    state.discovery_state = DiscoveryState::Disappeared;
                    state.addr.do_send(UpdateDiscoveryState(DiscoveryState::Disappeared));
                }
            });
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

//////////////////////////////////////////////////////////////////////////////////////////////////
// ClosingPeerConnection /////////////////////////////////////////////////////////////////////////

/// A message type used to indicate that a peer connection is closing.
#[derive(Message)]
pub(self) struct ClosingPeerConnection(pub PeerConnectionIdentifier);

/// The element used to identify a peer connection.
///
/// For outbound connections, before a handshake is finished, only the socket addr will be
/// available to identify the connection. For inbound connections, neither will be available at
/// first, and the connection can only be identified by the NodeID accurately, which will only be
/// available after a successful Railgun protocol handshake.
pub enum PeerConnectionIdentifier {
    SocketAddr(SocketAddr),
    NodeId(String),
}

impl Handler<ClosingPeerConnection> for Connections {
    type Result = ();

    /// Handle messages from peer connections indicating that the peer connection is closing.
    fn handle(&mut self, _msg: ClosingPeerConnection, _ctx: &mut Self::Context) {
        // TODO: remove peer socket addr from connections table.
    }
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// OutboundMessage ///////////////////////////////////////////////////////////////////////////////

/// A wrapper type for outbound messages destined for a specific peer.
///
/// The parent connections actor will receive messages from other higher-level actors to have
/// messages sent to specific destinations by Node ID. This same message instance will then be
/// forwarded to a specific child actor responsible for the target socket.
///
/// TODO: this should be renamed to `OutboundRequest` and should match the protocol defined in
/// `docs/internals/networking.md`. WsToPeer & WsFromPeer need to implement message handlers for
/// this message type as well, as they are responsible for actually flushing the message to the socket.
///
/// TODO: update this to wrap the internal API request frame enum & a node ID. Other higher-level
/// actors will send specific variants to the connections actor along with a target node ID, and
/// the connections actor will send it to the child actor responsible for the target socket. The
/// child actor will then wrap the frame in a Request API frame along with any needed metadata.
#[derive(Message)]
pub(self) struct OutboundMessage(pub Message);
