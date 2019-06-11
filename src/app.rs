use std::sync::Arc;

use actix::prelude::*;
use log::{error, info};

use crate::{
    config::Config,
    networking::{Network},
    db::{Storage},
};

/// The central Railgun actor.
///
/// This actor is the central control unit of a Railgun node, it is the brain. It is responsible
/// for spawning all other actors of the system. It implements the core behaviors of a Railgun
/// node either directly, or by way of communicating with the other actors of the system.
///
/// The networking layer (the `Network` actor) passes inbound network frames from peers and
/// connected clients to this actor for decision making on how to handle the received frames.
/// Some of the time the received frames will simply be passed off to one of the other actors,
/// such as the Raft or Database actors.
///
/// Though the `Network` actor will only pass inbound network frames to this actor, other actors
/// have direct access to the `Network` actor and may directly send outbound network frames to
/// it. The interface for sending a request to a peer node, for example, returns a future which
/// will resolve with the response from the peer or a timeout (which is configurable). This
/// provides a uniform interface for handling high-level logic on network frame routing within
/// this system, but gives actors direct access to the network stack for sending messages to peers
/// and clients.
pub struct App;

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
        let storage = Storage::new(app.clone(), &*config).unwrap_or_else(|err| {
            error!("Error initializing the system database. {}", err);
            std::process::exit(1);
        });
        let nodeid = storage.node_id();
        let _storageaddr = SyncArbiter::start(3, move || storage.clone()); // TODO: probably use `num_cores` crate.

        // Boot the network actor on a dedicated thread. Serves on dedicated threadpool.
        let (net_arb, net_cfg, net_app, net_nodeid) = (Arbiter::new(), config.clone(), app.clone(), nodeid.clone());
        let _conns_addr = Network::start_in_arbiter(&net_arb, move |net_ctx| {
            Network::new(net_ctx, net_app, net_nodeid, net_cfg)
        });

        // // Boot the consensus actor on a dedicated thread.
        // let (cns_arb, streams_arb) = (Arbiter::new(), Arbiter::new());
        // let cns = Consensus::new(app.clone(), nodeid, raftstore, cluster_state, streams_arb, ssaddr, config.clone()).unwrap_or_else(|err| {
        //     error!("Error initializing the consensus system. {}", err);
        //     std::process::exit(1);
        // });
        // let _cns_addr = Consensus::start_in_arbiter(&cns_arb, move |_| cns);

        info!("Railgun is firing on 0.0.0.0:{}!", &config.port);
        App
    }
}

impl Actor for App {
    type Context = Context<Self>;
}

// TODO: setup handler for inbound network frames from `Network` actor.
