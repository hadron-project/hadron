use std::sync::Arc;

use anyhow::{Context, Result};
use tracing_subscriber::prelude::*;

use hadrond::{App, Config};

#[tokio::main]
async fn main() -> Result<()> {
    // Setup tracing/logging system.
    tracing_subscriber::registry()
        // Filter spans based on the RUST_LOG env var.
        .with(tracing_subscriber::EnvFilter::from_default_env())
        // Send a copy of all spans to stdout in compact form.
        .with(
            tracing_subscriber::fmt::layer()
                .with_thread_names(true)
                .with_target(false)
                .with_level(true)
                .with_ansi(true)
        )
        // Install this registry as the global tracing registry.
        .try_init()
        .context("error initializing logging/tracing system")?;

    tracing::info!("starting hadron");
    let config = Arc::new(Config::new());
    let app_handle = App::new(config).await?.spawn();
    let _res = app_handle.await; // TODO: handle this.

    Ok(())
}
