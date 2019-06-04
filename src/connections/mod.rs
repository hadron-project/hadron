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
    time::Duration,
};

use actix::prelude::*;
use actix_web::{
    App, Error, HttpServer, HttpRequest, HttpResponse,
    dev::Server,
    web,
};
use actix_web_actors::ws;
use futures::future::err as fut_err;
use log::{debug, error};

use crate::{
    app,
    proto::{peer},
    common::NodeId,
    config::Config,
    connections::{
        from_peer::WsFromPeer,
        to_peer::{DiscoveryState, UpdateDiscoveryState, WsToPeer},
    },
    discovery::{
        Discovery, ObservedPeersChangeset,
    },
};

/// The interval at which heartbeats are sent to peer nodes.
pub(self) const PEER_HB_INTERVAL: Duration = Duration::from_secs(2);

/// The amount of time which is allowed to elapse between successful heartbeats before a
/// connection is reckoned as being dead between peer nodes.
pub(self) const PEER_HB_THRESHOLD: Duration = Duration::from_secs(10);

/// The amount of time which is allowed to elapse between a handshake request/response cycle.
pub(self) const PEER_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(2);

//////////////////////////////////////////////////////////////////////////////////////////////////
// ServerState ///////////////////////////////////////////////////////////////////////////////////

/// A type used as a shared state context for all WebSocket actor instances.
#[derive(Clone)]
pub(self) struct ServerState {
    pub parent: Addr<Connections>,
    pub node_id: NodeId,
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
// PeerAddr //////////////////////////////////////////////////////////////////////////////////////

/// A wrapper around the two possible peer connection actor types.
///
/// TODO: peer connections need to register themselves.
enum PeerAddr {
    ToPeer(Addr<WsToPeer>),
    FromPeer(Addr<WsFromPeer>),
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// Connections ///////////////////////////////////////////////////////////////////////////////////

/// An actor responsible for handling all network activity throughout the system.
///
/// See the README.md in this directory for additional information on actor responsibilities.
pub struct Connections {
    app: Addr<app::App>,
    node_id: NodeId,
    config: Arc<Config>,
    server: Option<Server>,
    socketaddr_to_peer: HashMap<SocketAddr, WsToPeerState>,
    routing_table: HashMap<NodeId, PeerAddr>,
    _discovery: Addr<Discovery>,
}

impl Connections {
    /// Create a new instance.
    ///
    /// This is expected to be called from within this actors `App::create` method which provides
    /// the context, and thus the address, of this actor. This is needed for spawning other actors
    /// and setting up proper communication channels.
    pub fn new(ctx: &mut Context<Self>, app: Addr<app::App>, node_id: NodeId, config: Arc<Config>) -> Self {

        // Boot the configured discovery system on a new dedicated thread.
        let (recipient, innercfg) = (ctx.address().recipient(), config.clone());
        let _discovery = Discovery::create(|innerctx|
            Discovery::new(innerctx, recipient, innercfg)
        );

        Self{
            app,
            node_id,
            config,
            server: None,
            socketaddr_to_peer: HashMap::new(),
            routing_table: HashMap::new(),
            _discovery,
        }
    }

    /// Build a new network server instance for use by this system.
    pub fn build_server(&self, ctx: &Context<Self>) -> Result<Server, ()> {
        let data = ServerState{parent: ctx.address(), node_id: self.node_id};
        let server = HttpServer::new(move || {
            App::new().data(data.clone())
                // This endpoint is used for internal client communication.
                .service(web::resource("/internal/").to(Self::handle_peer_connection))
                // .service(web::resource("").to(Self::handle_client_connection)) // TODO: client interface. See #6.
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
        ws::start(WsFromPeer::new(data.parent.clone(), data.node_id), &req, stream)
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
            let addr = WsToPeer::new(ctx.address(), self.node_id, socketaddr).start();
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
/// [See the Railgun peer handshake docs](docs/internals/peer-connection-management.md#railgun-peer-handshake)
/// for more details on exactly how the protocol works, and thus how it is to be implemented here.
///
/// Once the handshake is finished, the connection will be available for general use.
pub(self) enum PeerHandshakeState {
    /// The initial phase of the handshake protocol.
    Initial,
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
    /// This variant is used by `WsFromPeer` connections which do not have the SocketAddr of the peer.
    NodeId(NodeId),
    /// This variant is used by `WsToPeer` connections which only have the SocketAddr of the peer before the handshake is complete.
    SocketAddr(SocketAddr),
    /// This variant is used by `WsToPeer` connections after a successful handshake as both data elements will be available.
    SocketAddrAndId(SocketAddr, NodeId),
}

impl Handler<ClosingPeerConnection> for Connections {
    type Result = ();

    /// Handle messages from peer connections indicating that the peer connection is closing.
    fn handle(&mut self, msg: ClosingPeerConnection, _ctx: &mut Self::Context) {
        match msg.0 {
            PeerConnectionIdentifier::NodeId(id) => {
                self.routing_table.remove(&id);
            }
            PeerConnectionIdentifier::SocketAddr(addr) => {
                self.socketaddr_to_peer.remove(&addr);
            }
            PeerConnectionIdentifier::SocketAddrAndId(addr, id) => {
                self.routing_table.remove(&id);
                self.socketaddr_to_peer.remove(&addr);
            }
        }
    }
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// PeerConnectionLive ////////////////////////////////////////////////////////////////////////////

// TODO: get this wired up with child actors and handle here.

pub(self) struct PeerConnectionLive {
    /// The ID of the peer with which the connection is now live.
    peer_id: NodeId,

    /// Routing info coming from the newly connected peer.
    #[allow(dead_code)]
    routing_info: String,

    /// The address of the actor which is responsible for the new connection.
    addr: PeerAddr,
}

impl Message for PeerConnectionLive {
    type Result = Result<(), peer::api::Disconnect>;
}

impl Handler<PeerConnectionLive> for Connections {
    type Result = Result<(), peer::api::Disconnect>;

    /// Handle messages from child actors indicating that their connections are now live.
    ///
    /// This routine is responsible for a checking to ensure that the newly connected peer doesn't
    /// already have a connection. If it does, it will respond to the caller with an error
    /// indicating that such is the case.
    ///
    /// It will also update this actor's internal state to map the peer's node ID to the actor's
    /// address for message routing, and will propagate the routing of the newly connected peer
    /// to the App actor for high-level controls.
    fn handle(&mut self, msg: PeerConnectionLive, _ctx: &mut Self::Context) -> Self::Result {
        // If a connection already exists to the target peer, then this connection is invalid.
        if self.routing_table.contains_key(&msg.peer_id) {
            debug!("Connection with peer {} already established.", &msg.peer_id);
            return Err(peer::api::Disconnect::ConnectionInvalid);
        }

        // Update routing table with new information.
        debug!("Connection with peer {} now live.", &msg.peer_id);
        self.routing_table.insert(msg.peer_id, msg.addr);
        for peer in self.routing_table.keys() {
            debug!("Is connected to: {}", peer);
        }

        // Propagate new routing info.
        // TODO: impl this.

        Ok(())
    }
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// OutboundPeerRequest ///////////////////////////////////////////////////////////////////////////

/// A wrapper type for outbound requests destined for a specific peer.
///
/// The parent connections actor will receive messages from other higher-level actors to have
/// messages sent to specific destinations by Node ID. This same message instance will then be
/// forwarded to a specific child actor responsible for the target socket.
pub struct OutboundPeerRequest {
    pub request: peer::api::Request,
    pub target_node: NodeId,
    pub timeout: Duration,
}

impl actix::Message for OutboundPeerRequest {
    type Result = Result<peer::api::Response, ()>;
}

impl Handler<OutboundPeerRequest> for Connections {
    type Result = ResponseFuture<peer::api::Response, ()>;

    /// Handle requests to send outbound messages to a connected peer.
    fn handle(&mut self, msg: OutboundPeerRequest, _ctx: &mut Self::Context) -> Self::Result {
        // Get a reference to actor which is holding the connection to the target node.
        let addr = match self.routing_table.get(&msg.target_node) {
            None => {
                error!("Attempted to send an outbound peer request to a peer which is not registered in the connections routing table.");
                return Box::new(fut_err(()));
            }
            Some(addr) => addr,
        };

        // Send the outbound request to the target node.
        match addr {
            PeerAddr::FromPeer(iaddr) => Box::new(iaddr.send(msg)
                .map_err(|_| ())
                .and_then(|res| res)),
            PeerAddr::ToPeer(iaddr) => Box::new(iaddr.send(msg)
                .map_err(|_| ())
                .and_then(|res| res)),
        }
    }
}

// TODO: probably something like this.
// #[derive(Message)]
// pub(self) struct OutboundPeerResponse {
//     pub frame: PeerFrame,
//     pub target_node: String,
// }

// TODO: probably something like this.
// #[derive(Message)]
// pub(self) struct OutboundClientResponse {
//     pub frame: ClientFrame,
//     pub target_client: String,
// }

/// A message type wrapping an inbound peer API request along with its metadata.
#[derive(Message)]
pub struct InboundPeerRequest(peer::api::Request, peer::api::Meta);

impl Handler<InboundPeerRequest> for Connections {
    type Result = ();

    /// Handle inbound peer API requests.
    fn handle(&mut self, _msg: InboundPeerRequest, _ctx: &mut Self::Context) {
        // TODO: handle sending this request over to the App actor for high-level handling.
    }
}
