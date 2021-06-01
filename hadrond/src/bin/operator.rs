//! The Hadron operator.

use std::io::Write;
use std::sync::Arc;

use anyhow::{Context, Result};
use tracing_subscriber::prelude::*;

use hadrond::{AppServer, Config};

#[tokio::main]
async fn main() -> Result<()> {
    // Setup tracing/logging system.
    tracing_subscriber::registry()
        // Filter spans based on the RUST_LOG env var.
        .with(tracing_subscriber::EnvFilter::from_default_env())
        // Send a copy of all spans to stdout in compact form.
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(true)
                .with_level(true)
                .with_ansi(true)
        )
        // Install this registry as the global tracing registry.
        .try_init()
        .context("error initializing logging/tracing system")?;

    let cfg = Arc::new(Config::new());
    tracing::info!(
        client_port = %cfg.client_port,
        node_name = %cfg.node_name,
        repl_set_name = %cfg.repl_set_name,
        leader_name = %cfg.leader_name,
        metadata_repl_set_name = %cfg.metadata_repl_set_name,
        storage_data_path = %cfg.storage_data_path,
        "starting hadron",
    );
    if let Err(err) = AppServer::new(cfg).await?.spawn().await {
        tracing::error!(error = ?err);
    }

    // Ensure any pending output is flushed.
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();

    Ok(())
}
