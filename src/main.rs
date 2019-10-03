use std::{
    sync::Arc,
};

use actix::prelude::*;
use env_logger;

use railgun::{App, Config};

fn main() {
    // Build the system arbiter.
    let sys = actix::System::new("railgun");

    // Parse runtime config.
    let config = Arc::new(Config::new());

    // Initialize the logging system.
    env_logger::Builder::new().default_format().parse_filters(&config.logging).init();

    // Create the core Railgun actor. This takes care of spawning the rest of the system.
    App::create(move |ctx| App::new(ctx, config));

    let _ = sys.run(); // This blocks. Actix automatically handles unix signals for termination & graceful shutdown.
}
