//! The Hadron stream controller.

mod app;
mod config;
#[cfg(test)]
mod config_test;
mod database;
mod error;
#[cfg(test)]
mod fixtures;
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
use std::mem::MaybeUninit;
use std::sync::{Arc, Once};

use anyhow::{Context, Result};
use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusRecorder};
use tokio::sync::broadcast;
use tracing_subscriber::prelude::*;

use crate::app::App;
use crate::config::Config;
use hadron_core::prom::register_proc_metrics;

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
        stream = %cfg.stream,
        namespace = %cfg.namespace,
        pod_name = %cfg.pod_name,
        partition = %cfg.partition,
        storage_data_path = %cfg.storage_data_path,
        retention_policy_strategy = ?cfg.retention_policy.strategy,
        retention_policy_seconds = ?cfg.retention_policy.retention_seconds,
        "starting Hadron Stream controller",
    );
    let (shutdown_tx, _) = broadcast::channel(1);
    if let Err(err) = App::new(cfg, shutdown_tx.clone()).await?.spawn().await {
        tracing::error!(error = ?err);
        let _res = shutdown_tx.send(());
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
                    // NOTE: see https://github.com/hadron-project/hadron/issues/113 (needs to be improved).
                    .idle_timeout(metrics_util::MetricKindMask::ALL, Some(std::time::Duration::from_secs(60 * 60)))
                    .add_global_label("namespace", config.namespace.clone())
                    .add_global_label("stream", config.stream.clone())
                    .add_global_label("statefulset", config.statefulset.clone())
                    .add_global_label("pod_name", config.pod_name.clone())
                    .add_global_label("partition", format!("{}", config.partition))
                    .build(),
            );
        });
        RECORDER.assume_init_ref()
    }
}
