mod handlers;

use anyhow::{Context, Result};
use hadron::{Client, ClientCreds, Mode};
use sqlx::{migrate::Migrator, postgres::PgPoolOptions};
use tokio::signal::unix::{signal, SignalKind};
use tracing_subscriber::prelude::*;

static MIGRATOR: Migrator = sqlx::migrate!(); // Embeds the migrations in `./migrations`.

#[tokio::main]
async fn main() -> Result<()> {
    setup_tracing()?;
    tracing::info!("stream txp is coming online");

    // Extract needed env vars.
    let pg_url = std::env::var("POSTGRES_URL").context("required env var POSTGRES_URL not found")?;
    let hadron_url = std::env::var("HADRON_URL").context("required env var HADRON_URL not found")?;
    let hadron_token = std::env::var("HADRON_TOKEN").context("required env var HADRON_TOKEN not found")?;

    // Build the Hadron client.
    let creds = ClientCreds::new_with_token(&hadron_token)?;
    let client = Client::new(&hadron_url, Mode::Internal, creds).context("error constructing Hadron client instance")?;

    // Build the Postgres connection pool & execute migrations.
    let pg_pool = PgPoolOptions::new().max_connections(5).connect(&pg_url).await?;
    MIGRATOR
        .run(&pg_pool)
        .await
        .context("error running database migrations for Postgres")?;

    // Construct our Pipeline handlers and begin handling events from all stages.
    let deploy_service_handler = handlers::DeployServiceHandler::new(pg_pool.clone());
    let deploy_service_sub = client
        .pipeline("service-creation", "deploy-service", deploy_service_handler)
        .await?;
    let setup_billing_handler = handlers::SetupBillingHandler::new(pg_pool.clone());
    let setup_billing_sub = client
        .pipeline("service-creation", "setup-billing", setup_billing_handler)
        .await?;
    let setup_monitoring_handler = handlers::SetupMonitoringHandler::new(pg_pool.clone());
    let setup_monitoring_sub = client
        .pipeline("service-creation", "setup-monitoring", setup_monitoring_handler)
        .await?;
    let notify_user_handler = handlers::NotifyUserHandler::new(pg_pool.clone());
    let notify_user_sub = client
        .pipeline("service-creation", "notify-user", notify_user_handler)
        .await?;
    let cleanup_handler = handlers::CleanupHandler::new(pg_pool.clone());
    let cleanup_sub = client.pipeline("service-creation", "cleanup", cleanup_handler).await?;

    tracing::info!("pipeline stage subscribers started");
    wait_for_shutdown_signal().await?;
    deploy_service_sub.cancel().await;
    setup_billing_sub.cancel().await;
    setup_monitoring_sub.cancel().await;
    notify_user_sub.cancel().await;
    cleanup_sub.cancel().await;
    Ok(())
}

/// Setup tracing/logging system.
fn setup_tracing() -> Result<()> {
    tracing_subscriber::registry()
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
    Ok(())
}

/// Monitor for SIGINT or SIGTERM, shutting down when received.
async fn wait_for_shutdown_signal() -> Result<()> {
    let mut sigint = signal(SignalKind::interrupt()).context("error constructing SIGINT handler")?;
    let mut sigterm = signal(SignalKind::terminate()).context("error constructing SIGTERM handler")?;
    loop {
        tokio::select! {
            _ = sigint.recv() => {
                tracing::info!("received SIGINT, shutting down");
                break;
            }
            _ = sigterm.recv() => {
                tracing::info!("received SIGTERM, shutting down");
                break;
            }
        }
    }
    Ok(())
}
