use std::sync::Arc;

use anyhow::{Context, Result};
use hadron::{async_trait, Client, ClientCreds, Mode, StreamSubscribeResponse, SubscriberConfig, SubscriptionStartingPoint};
use sqlx::{migrate::Migrator, postgres::PgPoolOptions, PgPool};
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

    // Construct our Stream handler and begin handling events from the Stream.
    let handler = Arc::new(TxpHandler::new(pg_pool));
    let sub_config = SubscriberConfig {
        durable: true,
        max_batch_size: 1,
        starting_point: SubscriptionStartingPoint::Beginning,
    };
    let sub = client
        .subscribe("txp-demo", Some(sub_config), handler)
        .await
        .context("error building Stream subscription")?;

    tracing::info!("subscriber started");
    wait_for_shutdown_signal().await?;
    sub.cancel().await;
    Ok(())
}

/// The Stream handler used to implement our Transactional Processing system.
struct TxpHandler {
    pg_pool: PgPool,
}

impl TxpHandler {
    /// Create a new instance.
    fn new(pg_pool: PgPool) -> Self {
        Self { pg_pool }
    }
}

#[async_trait]
impl hadron::StreamHandler for TxpHandler {
    #[tracing::instrument(level = "info", skip(self, payload))]
    async fn handle(&self, payload: StreamSubscribeResponse) -> Result<()> {
        // Open a new transaction with our database.
        let mut tx = self.pg_pool.begin().await.context("error starting database transaction")?;

        // For each event in our batch (currently set to max 1 in this example, but can be set
        // much higher), materialize the event (id, source) into the `in_table` to ensure we
        // have not yet processed the event.
        for event in payload.batch {
            // Materialize into `in_table` and check for PK error. If so, continue to next event.
            let res = sqlx::query!("INSERT INTO in_table (id, source) VALUES ($1, $2);", &event.id, &event.source)
                .execute(&mut tx)
                .await;
            match res {
                Ok(_) => (),
                // Check for `unique_violation` (https://www.postgresql.org/docs/current/errcodes-appendix.html).
                // If so, then we've processed this event before, so skip it.
                Err(sqlx::Error::Database(err)) if err.code().as_deref() == Some("23505") => continue,
                Err(err) => return Err(err).context("error received while materializing event (id, source) into `in_table`"),
            }

            // Time for BUSINESS LOGIC! Do whatever our microservice needs to do here.
            // Make sure to use the same transaction for database interaction!
        }

        tx.commit().await.context("error committing database transaction")?;
        Ok(())
    }
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
