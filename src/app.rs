use std::{
    sync::Arc,
};

use actix::prelude::*;
use log::{error, info};

use crate::{
    config::Config,
    connections::{Connections},
    db::Database,
};

/// The central Railgun actor.
///
/// This actor is the central control unit of a Railgun node, it is the brain. It is responsible
/// for spawning all other actors of the system. It implements the core behaviors of a Railgun
/// node either directly, or by way of communicating with the other actors of the system.
///
/// The networking layer (the Connections actor) passes inbound network frames from peers and
/// connected clients to this actor for decision making on how to handle the received frames.
/// Some of the time the received frames will simply be passed off to one of the other actors,
/// such as the Raft or Database actors.
///
/// Though the Connections actor will only pass inbound network frames to this actor, other actors
/// have direct access to the Connections actor and may directly send outbound network frames to
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

        // Boot the database system.
        let db = Database::new(app.clone(), &*config).unwrap_or_else(|err| {
            error!("Error initializing the system database. {}", err);
            std::process::exit(1);
        });
        let nodeid = db.node_id().clone();
        let _dbaddr = db.start();

        // Boot the connections actor on a dedicated thread. The network server will serve using
        // a didicated threadpool.
        // TODO: app actor needs to take addr of this actor for propagating inbound network frames.
        let (conns_arb, conns_cfg, conns_node_id) = (Arbiter::new(), config.clone(), nodeid.clone());
        let _conns_addr = Connections::start_in_arbiter(&conns_arb, |ctx| Connections::new(ctx, conns_node_id, conns_cfg));

        info!("Railgun is firing on 0.0.0.0:{}!", &config.port);
        App
    }
}

impl Actor for App {
    type Context = Context<Self>;
}
