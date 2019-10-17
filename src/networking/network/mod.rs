pub(in crate::networking) mod clients;
pub(in crate::networking) mod forwarding;
pub(in crate::networking) mod metrics;
pub(in crate::networking) mod peers;
pub(in crate::networking) mod raft;

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
use futures::sync::{mpsc, oneshot};
use log::{debug, error};

use crate::{
    NodeId,
    app::{AppDataResponse, AppDataError, InboundRaftRequest, UpdatePeerInfo, RgClientPayload},
    config::Config,
    networking::{
        client::{WsClient, WsClientServices},
        from_peer::{WsFromPeer, WsFromPeerServices},
        network::{
            clients::GetRoutingInfo,
            peers::{PeerAddr, WsToPeerState},
            forwarding::{ForwardToLeader},
        },
    },
    discovery::{Discovery},
    proto::{peer},
};

/// The interval at which heartbeats are sent to peer nodes.
pub(super) const PEER_HB_INTERVAL: Duration = Duration::from_secs(2);
/// The amount of time which is allowed to elapse between successful heartbeats before a
/// connection is reckoned as being dead between peer nodes.
pub(super) const PEER_HB_THRESHOLD: Duration = Duration::from_secs(10);
/// The amount of time which is allowed to elapse between a handshake request/response cycle.
pub(super) const PEER_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(2);

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
pub(super) struct ServerState {
    pub parent: Addr<Network>,
    pub services: NetworkServices,
    pub node_id: NodeId,
    pub config: Arc<Config>,
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// Network ///////////////////////////////////////////////////////////////////////////////////////

/// An actor responsible for handling all network activity throughout the system.
///
/// See the README.md in this directory for additional information on actor responsibilities.
pub struct Network {
    services: NetworkServices,
    /// The ID of this node.
    node_id: NodeId,
    /// Runtime config.
    config: Arc<Config>,
    /// The network server. Will always be populated once this actor starts.
    server: Option<Server>,
    /// A mapping of discovered peer IPs, which will eventually graduate to routing table entries.
    socketaddr_to_peer: HashMap<SocketAddr, WsToPeerState>,
    /// A routing table of all connected peers, by ID.
    routing_table: HashMap<NodeId, PeerAddr>,
    /// The address of the discovery actor.
    #[allow(dead_code)]
    discovery: Addr<Discovery>,
    /// The source of truth on all of this node's connected clients and their routing info.
    routing: peer::RoutingInfo,
    /// A buffer of client requests which need to be forwarded to the Raft leader when known.
    forwarding_buffer: HashMap<String, (ForwardToLeader, oneshot::Sender<Result<AppDataResponse, AppDataError>>)>,
    /// A value tracking the current Raft leader.
    ///
    /// This value will only ever be updated directly from the Raft metrics stream or a forwarding
    /// request from Raft itself.
    current_leader: Option<NodeId>,
    /// A stream for updating the tracked ID of the Raft leader based on metrics directly from Raft.
    leader_update_stream: mpsc::UnboundedSender<Option<NodeId>>,
}

impl Network {
    /// Create a new instance.
    ///
    /// This is expected to be called from within this actors `App::create` method which provides
    /// the context, and thus the address, of this actor. This is needed for spawning other actors
    /// and setting up proper communication channels.
    pub fn new(
        ctx: &mut Context<Self>, services: NetworkServices, node_id: NodeId, config: Arc<Config>,
        leader_update_stream: mpsc::UnboundedSender<Option<NodeId>>,
    ) -> Self {

        // Boot the configured discovery system on a new dedicated thread.
        let (recipient, innercfg) = (ctx.address().recipient(), config.clone());
        let discovery = Discovery::create(|innerctx|
            Discovery::new(innerctx, recipient, innercfg)
        );

        Network{
            services, node_id, config,
            server: None, current_leader: None,
            socketaddr_to_peer: HashMap::new(),
            routing_table: HashMap::new(),
            discovery,
            routing: Default::default(),
            forwarding_buffer: Default::default(),
            leader_update_stream,
        }
    }

    /// Build a new network server instance for use by this system.
    pub fn build_server(&self, ctx: &Context<Self>) -> Result<Server, ()> {
        let data = ServerState{parent: ctx.address(), services: self.services.clone(), node_id: self.node_id, config: self.config.clone()};
        let server = HttpServer::new(move || {
            App::new().data(data.clone())
                // This endpoint is used for internal client communication.
                .service(web::resource("/").to(Self::handle_client_connection))
                .service(web::resource("/internal/").to_async(Self::handle_peer_connection))
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
    fn handle_peer_connection(req: HttpRequest, stream: web::Payload, data: web::Data<ServerState>) -> impl Future<Item=HttpResponse, Error=Error> {
        debug!("Handling a new peer connection request.");
        let services = WsFromPeerServices::new(
            data.parent.clone().recipient(),
            data.parent.clone().recipient(),
            data.services.inbound_raft_request.clone(),
            data.parent.clone().recipient(),
        );
        data.parent.clone().send(GetRoutingInfo)
            .map_err(From::from).and_then(|res| res.map_err(From::from))
            .and_then(move |routing_info| {
                ws::start(WsFromPeer::new(services, data.node_id, routing_info), &req, stream)
            })
    }

    fn handle_client_connection(req: HttpRequest, stream: web::Payload, data: web::Data<ServerState>) -> Result<HttpResponse, Error> {
        debug!("Handling a new client connection request.");
        let services = WsClientServices::new(data.services.client_payload.clone(), data.parent.clone().recipient(), data.parent.clone().recipient());
        ws::start(WsClient::new(services, data.node_id, data.config.client_liveness_threshold()), &req, stream)
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
