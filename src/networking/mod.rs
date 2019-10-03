mod client;
mod from_peer;
mod metrics;
mod raft;
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
use futures::future::{
    Either,
    err as fut_err,
};
use log::{debug, error};

use crate::{
    NodeId,
    app::{InboundRaftRequest, UpdatePeerInfo, RgClientPayload},
    auth::{Claims, ClaimsV1},
    config::Config,
    networking::{
        client::{VerifyToken, WsClient, WsClientServices},
        from_peer::{WsFromPeer, WsFromPeerServices},
        to_peer::{DiscoveryState, UpdateDiscoveryState, WsToPeer, WsToPeerServices},
    },
    discovery::{
        Discovery, ObservedPeersChangeset,
    },
    proto::{client::api::ClientError, peer::{self, api}},
};

/// The interval at which heartbeats are sent to peer nodes.
pub(self) const PEER_HB_INTERVAL: Duration = Duration::from_secs(2);
/// The amount of time which is allowed to elapse between successful heartbeats before a
/// connection is reckoned as being dead between peer nodes.
pub(self) const PEER_HB_THRESHOLD: Duration = Duration::from_secs(10);
/// The amount of time which is allowed to elapse between a handshake request/response cycle.
pub(self) const PEER_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(2);

//////////////////////////////////////////////////////////////////////////////////////////////////
// NetworkServices  //////////////////////////////////////////////////////////////////////////////

/// All services needed by the `Network` actor.
#[derive(Clone)]
pub struct NetworkServices {
    pub client_payload: Recipient<RgClientPayload>,
    pub update_peer_info: Recipient<UpdatePeerInfo>,
    pub inbound_raft_request: Recipient<InboundRaftRequest>,
}

impl NetworkServices {
    /// Create a new instance.
    pub fn new(
        client_payload: Recipient<RgClientPayload>,
        update_peer_info: Recipient<UpdatePeerInfo>,
        inbound_raft_request: Recipient<InboundRaftRequest>,
    ) -> Self {
        Self{client_payload, update_peer_info, inbound_raft_request}
    }
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// ServerState ///////////////////////////////////////////////////////////////////////////////////

/// A type used as a shared state context for all WebSocket actor instances.
#[derive(Clone)]
pub(self) struct ServerState {
    pub parent: Addr<Network>,
    pub services: NetworkServices,
    pub node_id: NodeId,
    pub config: Arc<Config>,
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
// Network ///////////////////////////////////////////////////////////////////////////////////////

/// An actor responsible for handling all network activity throughout the system.
///
/// See the README.md in this directory for additional information on actor responsibilities.
pub struct Network {
    services: NetworkServices,
    node_id: NodeId,
    config: Arc<Config>,
    server: Option<Server>,
    socketaddr_to_peer: HashMap<SocketAddr, WsToPeerState>,
    routing_table: HashMap<NodeId, PeerAddr>,
    _discovery: Addr<Discovery>,
}

impl Network {
    /// Create a new instance.
    ///
    /// This is expected to be called from within this actors `App::create` method which provides
    /// the context, and thus the address, of this actor. This is needed for spawning other actors
    /// and setting up proper communication channels.
    pub fn new(ctx: &mut Context<Self>, services: NetworkServices, node_id: NodeId, config: Arc<Config>) -> Self {

        // Boot the configured discovery system on a new dedicated thread.
        let (recipient, innercfg) = (ctx.address().recipient(), config.clone());
        let _discovery = Discovery::create(|innerctx|
            Discovery::new(innerctx, recipient, innercfg)
        );

        Network{
            services, node_id, config,
            server: None,
            socketaddr_to_peer: HashMap::new(),
            routing_table: HashMap::new(),
            _discovery,
        }
    }

    /// Build a new network server instance for use by this system.
    pub fn build_server(&self, ctx: &Context<Self>) -> Result<Server, ()> {
        let data = ServerState{parent: ctx.address(), services: self.services.clone(), node_id: self.node_id, config: self.config.clone()};
        let server = HttpServer::new(move || {
            App::new().data(data.clone())
                // This endpoint is used for internal client communication.
                .service(web::resource("/internal/").to(Self::handle_peer_connection))
                .service(web::resource("").to(Self::handle_client_connection))
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
        let services = WsFromPeerServices::new(data.parent.clone().recipient(),
            data.services.inbound_raft_request.clone(),
            data.parent.clone().recipient(),
        );
        ws::start(WsFromPeer::new(services, data.node_id), &req, stream)
    }

    fn handle_client_connection(req: HttpRequest, stream: web::Payload, data: web::Data<ServerState>) -> Result<HttpResponse, Error> {
        debug!("Handling a new client connection request.");
        let services = WsClientServices::new(data.services.client_payload.clone(), data.parent.clone().recipient());
        ws::start(WsClient::new(services, data.node_id, data.config.client_liveness_threshold()), &req, stream)
    }

    pub(self) fn send_outbound_peer_request(&mut self, msg: OutboundPeerRequest, _: &mut Context<Self>) -> impl Future<Item=api::Response, Error=()> {
        // Get a reference to actor which is holding the connection to the target node.
        let addr = match self.routing_table.get(&msg.target_node) {
            None => {
                error!("Attempted to send an outbound peer request to a peer which is not registered in the connections routing table.");
                return Either::B(fut_err(()));
            }
            Some(addr) => addr,
        };

        // Send the outbound request to the target node.
        match addr {
            PeerAddr::FromPeer(iaddr) => Either::A(Either::A(iaddr.send(msg).map_err(|_| ()).and_then(|res| res))),
            PeerAddr::ToPeer(iaddr) => Either::A(Either::B(iaddr.send(msg).map_err(|_| ()).and_then(|res| res))),
        }
    }
}

impl Actor for Network {
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

impl Handler<ObservedPeersChangeset> for Network {
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
            let services = WsToPeerServices::new(ctx.address().recipient(), self.services.inbound_raft_request.clone(), ctx.address().recipient());
            let addr = WsToPeer::new(services, self.node_id, socketaddr).start();
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
// PeerConnectionIdentifier //////////////////////////////////////////////////////////////////////

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

//////////////////////////////////////////////////////////////////////////////////////////////////
// ClosingPeerConnection /////////////////////////////////////////////////////////////////////////

/// A message type used to indicate that a peer connection is closing.
#[derive(Message)]
pub struct ClosingPeerConnection(pub PeerConnectionIdentifier);

impl Handler<ClosingPeerConnection> for Network {
    type Result = ();

    /// Handle messages from peer connections indicating that the peer connection is closing.
    fn handle(&mut self, msg: ClosingPeerConnection, _ctx: &mut Self::Context) {
        match msg.0 {
            PeerConnectionIdentifier::NodeId(id) => {
                self.routing_table.remove(&id);
                let _ = self.services.update_peer_info.do_send(UpdatePeerInfo::Remove(id))
                    .map_err(|err| error!("Error sending `UpdatePeerInfo::Remove` from `Network` actor. {}", err));
            }
            PeerConnectionIdentifier::SocketAddr(addr) => {
                self.socketaddr_to_peer.remove(&addr);
            }
            PeerConnectionIdentifier::SocketAddrAndId(addr, id) => {
                self.routing_table.remove(&id);
                self.socketaddr_to_peer.remove(&addr);
                let _ = self.services.update_peer_info.do_send(UpdatePeerInfo::Remove(id))
                    .map_err(|err| error!("Error sending `UpdatePeerInfo::Remove` from `Network` actor. {}", err));
            }
        }
    }
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// PeerConnectionLive ////////////////////////////////////////////////////////////////////////////

/// A message indicating that a connection with a specific peer is now live.
pub struct PeerConnectionLive {
    /// The ID of the peer with which the connection is now live.
    peer_id: NodeId,

    /// Routing info coming from the newly connected peer.
    #[allow(dead_code)]
    routing_info: String,

    /// The address of the actor which is responsible for the new connection.
    addr: PeerAddr,
}

impl Message for PeerConnectionLive {
    type Result = ();
}

impl Handler<PeerConnectionLive> for Network {
    type Result = ();

    /// Handle messages from child actors indicating that their connections are now live.
    ///
    /// This routine is responsible for a checking to ensure that the newly connected peer doesn't
    /// already have a connection. If it does, it will sever the old connection, replacing it with
    /// the new connection.
    ///
    /// It will also update this actor's internal state to map the peer's node ID to the actor's
    /// address for message routing, and will propagate the routing info of the newly connected
    /// peer to the App actor for high-level controls.
    fn handle(&mut self, msg: PeerConnectionLive, _ctx: &mut Self::Context) {
        // Update routing table with new information.
        let old = self.routing_table.insert(msg.peer_id, msg.addr);

        // If an old connection has been replaced, disconnect it.
        match old {
            Some(PeerAddr::FromPeer(addr)) => addr.do_send(DisconnectPeer),
            Some(PeerAddr::ToPeer(addr)) => addr.do_send(DisconnectPeer),
            None => (),
        }
        self.routing_table.keys().for_each(|peer| debug!("Is connected to: {}", peer));

        // Update app instance with new connection info.
        let _ = self.services.update_peer_info.do_send(UpdatePeerInfo::Update{peer: msg.peer_id, routing_info: msg.routing_info})
            .map_err(|err| error!("Error sending `UpdatePeerInfo::Update` from `Network` actor. {}", err));
    }
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// DisconnectPeer ////////////////////////////////////////////////////////////////////////////////

/// A message instructing a peer connection to be severed.
#[derive(Message)]
pub(self) struct DisconnectPeer;

//////////////////////////////////////////////////////////////////////////////////////////////////
// OutboundPeerRequest ///////////////////////////////////////////////////////////////////////////

/// A wrapper type for outbound requests destined for a specific peer.
///
/// The parent `Network` actor will receive messages from other higher-level actors to have
/// messages sent to specific destinations by Node ID. Different message types are used by other
/// higher-level actors, as different actors may require different intnerfaces, but they should
/// all use this message type in their handlers to interact with the peer connection actors.
pub struct OutboundPeerRequest {
    pub request: peer::api::Request,
    pub target_node: NodeId,
    pub timeout: Duration,
}

impl actix::Message for OutboundPeerRequest {
    type Result = Result<peer::api::Response, ()>;
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// VerifyToken ///////////////////////////////////////////////////////////////////////////////////

impl Handler<VerifyToken> for Network {
    type Result = ResponseActFuture<Self, Claims, ClientError>;

    fn handle(&mut self, msg: VerifyToken, _ctx: &mut Context<Self>) -> Self::Result {
        // TODO: implement this. See #24.
        if msg.0.len() != 0 {
            Box::new(fut::err(ClientError::new_unauthorized()))
        } else {
            Box::new(fut::ok(Claims::V1(ClaimsV1{all: true, grants: Vec::new()})))
        }
    }
}
