//! The Hadron stream controller.

mod app;
mod config;
mod database;
mod error;
mod futures;
mod grpc;
mod models;
mod pipeline;
mod server;
mod stream;
mod utils;
#[cfg(test)]
mod utils_test;
mod watchers;

use std::io::Write;
use std::sync::Arc;

use anyhow::{Context, Result};
use tracing_subscriber::prelude::*;

use crate::app::App;
use crate::config::Config;

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

    let cfg = Arc::new(Config::new()?);
    tracing::info!(
        client_port = %cfg.client_port,
        stream = %cfg.stream,
        namespace = %cfg.namespace,
        pod_name = %cfg.pod_name,
        partition = %cfg.partition,
        storage_data_path = %cfg.storage_data_path,
        "starting Hadron Stream controller",
    );
    if let Err(err) = App::new(cfg).await?.spawn().await {
        tracing::error!(error = ?err);
    }

    // Ensure any pending output is flushed.
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();

    Ok(())
}
