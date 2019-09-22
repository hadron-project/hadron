use std::sync::Arc;

use actix::prelude::*;
use actix_raft::{
    Raft,
    admin::{InitWithConfig},
};
use log::{error, info};

use crate::{
    NodeId,
    config::Config,
    networking::{Network},
    db::{AppData, Storage},
    proto::client::api::ClientError,
};

/// This application's concrete Raft type.
type AppRaft = Raft<AppData, ClientError, Network, Storage>;

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
        let storage = Storage::new(&config.db_path).unwrap_or_else(|err| {
            error!("Error initializing the system database. {}", err);
            std::process::exit(1);
        });
        let nodeid = storage.node_id();
        let storage_arb = Arbiter::new();
        let storage_addr = Storage::start_in_arbiter(&storage_arb, move |_| storage);

        // Boot the network actor on a dedicated thread. Serves on dedicated threadpool.
        let (net_arb, net_cfg, net_app, net_nodeid) = (Arbiter::new(), config.clone(), app.clone(), nodeid.clone());
        let net_addr = Network::start_in_arbiter(&net_arb, move |net_ctx| {
            Network::new(net_ctx, net_app, net_nodeid, net_cfg)
        });
        let metrics_receiver = net_addr.clone().recipient();

        // Boot the consensus actor on a dedicated thread.
        let raft_cfg = actix_raft::Config::build(config.snapshot_dir()).validate().unwrap_or_else(|err| {
            error!("Error building Raft config. {}", err);
            std::process::exit(1);
        });
        let raft_arb = Arbiter::new();
        let raft = AppRaft::new(nodeid.clone(), raft_cfg, net_addr.clone(), storage_addr.clone(), metrics_receiver);
        let raft_addr = AppRaft::start_in_arbiter(&raft_arb, move |_| raft);

        info!("Railgun is firing on all interfaces on port {}!", &config.port);
        App{
            id: nodeid,
            config,
            _storage: storage_addr,
            _network: net_addr,
            raft: raft_addr,
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
        let f = self.raft.send(InitWithConfig::new(vec![self.id]))
            .map_err(|err| {
                error!("Error sending InitWithConfig command. {}", err)
            })
            .and_then(|res| futures::future::result(res).map_err(|err| {
                error!("Error from InitWithConfig command. {:?}", err)
            }));
        ctx.spawn(fut::wrap_future(f));
    }
}

// TODO: setup handler for inbound network frames from `Network` actor.
