use std::{
    collections::BTreeMap,
    sync::Arc,
};

use actix::prelude::*;
use actix_raft::{
    Raft,
    admin::{InitWithConfig},
    config::SnapshotPolicy,
    messages::{
        AppendEntriesRequest,
        ClientPayload,
        ClientPayloadResponse,
        ClientError as ClientPayloadError,
        Entry, EntryNormal, EntryPayload,
        VoteRequest,
        InstallSnapshotRequest,
    },
};
use log::{error, info};

use crate::{
    NodeId,
    config::Config,
    db::Storage,
    networking::{
        Network, NetworkServices,
    },
    proto::{
        client::api::{
            AckPipelineRequest, AckStreamRequest, ClientError,
            EnsurePipelineRequest, EnsureRpcEndpointRequest, EnsureStreamRequest,
            PubStreamRequest, SubPipelineRequest, SubStreamRequest,
            UnsubPipelineRequest, UnsubStreamRequest,
        },
        peer,
    },
};

/// This application's concrete Raft type.
type AppRaft = Raft<AppData, AppDataResponse, ClientError, Network, Storage>;
pub type RgEntry = Entry<AppData>;
pub type RgEntryPayload = EntryPayload<AppData>;
pub type RgEntryNormal = EntryNormal<AppData>;
pub type RgClientPayload = ClientPayload<AppData, AppDataResponse, ClientError>;
pub type RgClientPayloadError = ClientPayloadError<AppData, AppDataResponse, ClientError>;
pub type RgClientPayloadResponse = ClientPayloadResponse<AppDataResponse>;

//////////////////////////////////////////////////////////////////////////////////////////////////
// App ///////////////////////////////////////////////////////////////////////////////////////////

/// The central Railgun actor.
///
/// This actor is the central control unit of a Railgun node, it is the brain. It is responsible
/// for spawning all other actors of the system. It implements the core behaviors of a Railgun
/// node either directly, or by way of communicating with the other actors of the system.
///
/// The networking layer (the `Network` actor) passes inbound network frames from peers and
/// connected clients to this actor for decision making on how to handle the received frames.
/// Some of the time the received frames will simply be passed off to one of the other actors,
/// such as the Raft or storage actors.
///
/// Though the `Network` actor will only pass inbound network frames to this actor, other actors
/// have direct access to the `Network` actor and may directly send outbound network frames to
/// it. The interface for sending a request to a peer node, for example, returns a future which
/// will resolve with the response from the peer or a timeout (which is configurable). This
/// provides a uniform interface for handling high-level logic on network frame routing within
/// this system, but gives actors direct access to the network stack for sending messages to peers
/// and clients.
pub struct App {
    id: NodeId,
    config: Arc<Config>,
    _storage: Addr<Storage>,
    _network: Addr<Network>,
    raft: Addr<AppRaft>,
    peers: BTreeMap<NodeId, String>,
}

impl App {
    /// Create a new instance.
    ///
    /// This is expected to be called from within this actors `App::create` method which provides
    /// the context, and thus the address, of this actor. This is needed for spawning other actors
    /// and setting up proper communication channels.
    pub fn new(ctx: &mut Context<Self>, config: Arc<Config>) -> Self {
        info!("Booting the Railgun application.");

        // The address of self for spawned child actors to communicate back to this actor.
        let app = ctx.address();

        // Instantiate the Raft storage system & start it.
        let storage = Storage::new(&config.storage_db_path).unwrap_or_else(|err| {
            error!("Error initializing the system database. {}", err);
            std::process::exit(1);
        });
        let nodeid = storage.node_id();
        let storage_arb = Arbiter::new();
        let storage_addr = Storage::start_in_arbiter(&storage_arb, move |_| storage);

        // Boot the network actor on a dedicated thread. Serves on dedicated threadpool.
        let (net_arb, net_cfg, net_app, net_nodeid) = (Arbiter::new(), config.clone(), app.clone(), nodeid.clone());
        let net_addr = Network::start_in_arbiter(&net_arb, move |net_ctx| {
            let services = NetworkServices::new(app.clone().recipient(), net_app.clone().recipient(), net_app.clone().recipient());
            Network::new(net_ctx, services, net_nodeid, net_cfg)
        });
        let metrics_receiver = net_addr.clone().recipient();

        // Boot the consensus actor on a dedicated thread.
        let raft_cfg = actix_raft::Config::build(config.storage_snapshot_dir())
            .heartbeat_interval(config.raft_heartbeat_interval_millis)
            .election_timeout_max(config.raft_election_timeout_max)
            .election_timeout_min(config.raft_election_timeout_min)
            .snapshot_policy(SnapshotPolicy::Disabled)
            .validate().unwrap_or_else(|err| {
                error!("Error building Raft config. {}", err);
                std::process::exit(1);
            });
        let raft_arb = Arbiter::new();
        let raft = AppRaft::new(nodeid.clone(), raft_cfg, net_addr.clone(), storage_addr.clone(), metrics_receiver);
        let raft_addr = AppRaft::start_in_arbiter(&raft_arb, move |_| raft);

        info!("Railgun is firing on all interfaces at port {}!", &config.port);
        App{
            id: nodeid,
            config,
            _storage: storage_addr,
            _network: net_addr,
            raft: raft_addr,
            peers: BTreeMap::new(),
        }
    }
}

impl Actor for App {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        // Spawn a delayed function for issuing the initial cluster formation command.
        ctx.run_later(self.config.initial_cluster_formation_delay(), |act, ctx| act.initial_cluster_formation(ctx));
    }
}

impl App {
    /// Issue the initial cluster formation command to this node.
    fn initial_cluster_formation(&mut self, ctx: &mut Context<Self>) {
        let mut cluster_members: Vec<NodeId> = self.peers.keys().copied().collect();
        cluster_members.push(self.id);

        let f = self.raft.send(InitWithConfig::new(cluster_members))
            .map_err(|err| {
                error!("Error sending InitWithConfig command. {}", err)
            })
            .and_then(|res| futures::future::result(res).map_err(|err| {
                error!("Error from InitWithConfig command. {:?}", err)
            }));
        ctx.spawn(fut::wrap_future(f));
    }

    fn handle_raft_append_entries_request(&mut self, req: AppendEntriesRequest<AppData>, _: &mut Context<Self>) -> impl ActorFuture<Actor=Self, Item=peer::api::RaftResponse, Error=peer::api::Error> {
        fut::wrap_future(self.raft.send(req)
            .map_err(|err| {
                error!("Error handling Raft AppendEntriesRequest. {}", err);
                peer::api::Error::Internal
            }))
            .and_then(|res, _, _| fut::result(res).map_err(|_, _, _| peer::api::Error::Internal))
            .and_then(|res, _, _| fut::result(bincode::serialize(&res).map_err(|err| {
                error!("Error serializing AppendEntriesResponse. {}", err);
                peer::api::Error::Internal
            })))
            .map(|data, _, _| peer::api::RaftResponse{payload: Some(peer::api::raft_response::Payload::AppendEntries(data))})
    }

    fn handle_raft_vote_request(&mut self, req: VoteRequest, _: &mut Context<Self>) -> impl ActorFuture<Actor=Self, Item=peer::api::RaftResponse, Error=peer::api::Error> {
        fut::wrap_future(self.raft.send(req)
            .map_err(|err| {
                error!("Error handling Raft VoteRequest. {}", err);
                peer::api::Error::Internal
            }))
            .and_then(|res, _, _| fut::result(res).map_err(|_, _, _| peer::api::Error::Internal))
            .and_then(|res, _, _| fut::result(bincode::serialize(&res).map_err(|err| {
                error!("Error serializing VoteResponse. {}", err);
                peer::api::Error::Internal
            })))
            .map(|data, _, _| peer::api::RaftResponse{payload: Some(peer::api::raft_response::Payload::Vote(data))})
    }

    fn handle_raft_install_snapshot_request(&mut self, req: InstallSnapshotRequest, _: &mut Context<Self>) -> impl ActorFuture<Actor=Self, Item=peer::api::RaftResponse, Error=peer::api::Error> {
        fut::wrap_future(self.raft.send(req)
            .map_err(|err| {
                error!("Error handling Raft InstallSnapshotRequest. {}", err);
                peer::api::Error::Internal
            }))
            .and_then(|res, _, _| fut::result(res).map_err(|_, _, _| peer::api::Error::Internal))
            .and_then(|res, _, _| fut::result(bincode::serialize(&res).map_err(|err| {
                error!("Error serializing InstallSnapshotResponse. {}", err);
                peer::api::Error::Internal
            })))
            .map(|data, _, _| peer::api::RaftResponse{payload: Some(peer::api::raft_response::Payload::InstallSnapshot(data))})
    }
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// UpdatePeerInfo ////////////////////////////////////////////////////////////////////////////////

/// A message indicating an update to a peer's connection info.
#[derive(Message)]
pub enum UpdatePeerInfo {
    Update{
        peer: NodeId,
        routing_info: String,
    },
    Remove(NodeId),
}

impl Handler<UpdatePeerInfo> for App {
    type Result = ();

    fn handle(&mut self, msg: UpdatePeerInfo, _: &mut Context<Self>) -> Self::Result {
        match msg {
            UpdatePeerInfo::Remove(id) => {
                self.peers.remove(&id);
            }
            UpdatePeerInfo::Update{peer, routing_info} => {
                self.peers.insert(peer, routing_info);
            }
        }
    }
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// ClientPayload /////////////////////////////////////////////////////////////////////////////////

impl Handler<RgClientPayload> for App {
    type Result = ResponseFuture<RgClientPayloadResponse, RgClientPayloadError>;

    fn handle(&mut self, msg: RgClientPayload, _ctx: &mut Context<Self>) -> Self::Result {
        Box::new(self.raft.send(msg)
            .map_err(|_| ClientPayloadError::Application(ClientError::new_internal()))
            .and_then(|res| res))
    }
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// InboundRaftRequest ////////////////////////////////////////////////////////////////////////////

/// A message type wrapping an inbound peer API request along with its metadata.
pub struct InboundRaftRequest(pub peer::api::RaftRequest, pub peer::api::Meta);

impl Message for InboundRaftRequest {
    type Result = Result<peer::api::RaftResponse, peer::api::Error>;
}

impl Handler<InboundRaftRequest> for App {
    type Result = ResponseActFuture<Self, peer::api::RaftResponse, peer::api::Error>;

    /// Handle inbound peer API requests.
    fn handle(&mut self, msg: InboundRaftRequest, _ctx: &mut Self::Context) -> Self::Result {
        let (req, _meta) = (msg.0, msg.1);
        use peer::api::raft_request::Payload;
        match req.payload {
            Some(Payload::AppendEntries(data)) => Box::new(fut::result(bincode::deserialize::<AppendEntriesRequest<AppData>>(data.as_slice())
                .map_err(|err| {
                    error!("Error deserializing inbound AppendEntriesRequest. {}", err);
                    peer::api::Error::Internal
                }))
                .and_then(|req, act: &mut Self, ctx| act.handle_raft_append_entries_request(req, ctx))),
            Some(Payload::Vote(data)) => Box::new(fut::result(bincode::deserialize::<VoteRequest>(data.as_slice())
                .map_err(|err| {
                    error!("Error deserializing inbound VoteRequest. {}", err);
                    peer::api::Error::Internal
                }))
                .and_then(|req, act: &mut Self, ctx| act.handle_raft_vote_request(req, ctx))),
            Some(Payload::InstallSnapshot(data)) => Box::new(fut::result(bincode::deserialize::<InstallSnapshotRequest>(data.as_slice())
                .map_err(|err| {
                    error!("Error deserializing inbound InstallSnapshotRequest. {}", err);
                    peer::api::Error::Internal
                }))
                .and_then(|req, act: &mut Self, ctx| act.handle_raft_install_snapshot_request(req, ctx))),
            _ => {
                error!("Unknown Raft request variant received.");
                Box::new(fut::err(peer::api::Error::Internal))
            }
        }
    }
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// AppData ///////////////////////////////////////////////////////////////////////////////////////

/// All data variants which are persisted via Raft.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum AppData {
    PubStream(PubStreamRequest),
    SubStream(SubStreamRequest),
    SubPipeline(SubPipelineRequest),
    UnsubStream(UnsubStreamRequest),
    UnsubPipeline(UnsubPipelineRequest),
    EnsureRpcEndpoint(EnsureRpcEndpointRequest),
    EnsureStream(EnsureStreamRequest),
    EnsurePipeline(EnsurePipelineRequest),
    AckStream(AckStreamRequest),
    AckPipeline(AckPipelineRequest),
}

impl actix_raft::AppData for AppData {}

impl From<PubStreamRequest> for AppData {
    fn from(src: PubStreamRequest) -> Self {
        AppData::PubStream(src)
    }
}

impl From<SubStreamRequest> for AppData {
    fn from(src: SubStreamRequest) -> Self {
        AppData::SubStream(src)
    }
}

impl From<SubPipelineRequest> for AppData {
    fn from(src: SubPipelineRequest) -> Self {
        AppData::SubPipeline(src)
    }
}

impl From<UnsubStreamRequest> for AppData {
    fn from(src: UnsubStreamRequest) -> Self {
        AppData::UnsubStream(src)
    }
}

impl From<UnsubPipelineRequest> for AppData {
    fn from(src: UnsubPipelineRequest) -> Self {
        AppData::UnsubPipeline(src)
    }
}

impl From<EnsureRpcEndpointRequest> for AppData {
    fn from(src: EnsureRpcEndpointRequest) -> Self {
        AppData::EnsureRpcEndpoint(src)
    }
}

impl From<EnsureStreamRequest> for AppData {
    fn from(src: EnsureStreamRequest) -> Self {
        AppData::EnsureStream(src)
    }
}

impl From<EnsurePipelineRequest> for AppData {
    fn from(src: EnsurePipelineRequest) -> Self {
        AppData::EnsurePipeline(src)
    }
}

impl From<AckStreamRequest> for AppData {
    fn from(src: AckStreamRequest) -> Self {
        AppData::AckStream(src)
    }
}

impl From<AckPipelineRequest> for AppData {
    fn from(src: AckPipelineRequest) -> Self {
        AppData::AckPipeline(src)
    }
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// AppDataResponse ///////////////////////////////////////////////////////////////////////////////

/// Data response variants from applying entries to the Raft state machine.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum AppDataResponse {
    Noop,
    PubStream {
        /// The index of the published message on its stream.
        index: u64,
    },
    SubStream,
    SubPipeline,
    UnsubStream,
    UnsubPipeline,
    EnsureRpcEndpoint,
    EnsureStream,
    EnsurePipeline,
    AckStream,
    AckPipeline,
}

impl actix_raft::AppDataResponse for AppDataResponse {}
