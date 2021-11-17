//! The Hadron stream controller.

mod app;
mod config;
mod grpc;
mod k8s;
mod server;

use std::io::Write;
use std::mem::MaybeUninit;
use std::sync::{Arc, Once};

use anyhow::{Context, Result};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusRecorder};
use tracing_subscriber::prelude::*;

use crate::app::App;
use crate::config::Config;
use hadron_core::procmetrics::register_proc_metrics;

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
    let recorder = get_metrics_recorder(&cfg);
    metrics::set_recorder(recorder).context("error setting prometheus metrics recorder")?;
    register_proc_metrics();

    tracing::info!(
        client_port = %cfg.client_port,
        namespace = %cfg.namespace,
        "starting Hadron Operator",
    );
    if let Err(err) = App::new(cfg).await?.spawn().await {
        tracing::error!(error = ?err);
    }

    // Ensure any pending output is flushed.
    let _ = std::io::stdout().flush();
    let _ = std::io::stderr().flush();

    Ok(())
}

/// Get a handle to the metrics recorder, initializing it as needed.
pub fn get_metrics_recorder(config: &Config) -> &'static PrometheusRecorder {
    static mut RECORDER: MaybeUninit<PrometheusRecorder> = MaybeUninit::uninit();
    static ONCE: Once = Once::new();
    unsafe {
        ONCE.call_once(|| {
            RECORDER.write(
                PrometheusBuilder::new()
                    .add_global_label("namespace", config.namespace.clone())
                    .add_global_label("pod", config.pod_name.clone())
                    .build(),
            );
        });
        RECORDER.assume_init_ref()
    }
}
