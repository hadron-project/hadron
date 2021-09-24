use std::sync::Arc;

use anyhow::{Context, Result};
use hadron::{async_trait, Event, Message, PipelineSubscribeResponse};
use sqlx::PgPool;

/// Handler used for the `setup-monitoring` Pipeline stage.
pub struct SetupMonitoringHandler {
    pg_pool: PgPool,
}

impl SetupMonitoringHandler {
    /// Construct a new instance.
    pub fn new(pg_pool: PgPool) -> Arc<Self> {
        Arc::new(Self { pg_pool })
    }
}

#[async_trait]
impl hadron::PipelineHandler for SetupMonitoringHandler {
    #[tracing::instrument(level = "info", skip(self, payload))]
    async fn handle(&self, payload: PipelineSubscribeResponse) -> Result<Event> {
        // Open a new transaction with our database.
        let mut tx = self.pg_pool.begin().await.context("error starting database transaction")?;

        // Materialize the root event's (id, source) into the `in_table` along with the stage name.
        // This ensures that this stage has not yet been executed for this root event.
        let root_event = payload.root_event.unwrap_or_default();
        let opt_record = sqlx::query!(
            r#"SELECT event FROM in_table_pipelines
                WHERE id=$1 AND source=$2 AND stage=$3;"#,
            &root_event.id,
            &root_event.source,
            &payload.stage,
        )
        .fetch_optional(&mut tx)
        .await
        .context("error querying in_table_pipelines")?;
        if let Some(record) = opt_record {
            // Duplicate. Deserialize the event and return.
            let event = Event::decode(record.event.as_slice()).context("error decoding event from table row")?;
            let _res = tx.rollback().await.context("error rolling back transaction")?;
            return Ok(event);
        }

        // Time for BUSINESS LOGIC! Do whatever our microservice needs to do here.
        // This is the `deploy-service` handler, so we should probably deploy something. Within
        // this transaction, we can update state to track our progress within this task.

        // Once business logic has finished, we need to generate our output event, write it to our
        // in_table_pipelines along with `(id, source, stage)` to guard against duplicate processing.
        let output_event = Event::new(
            root_event.id.clone(),
            format!("{}/service-creation/setup-monitoring", &payload.stage),
            String::from("setup-monitoring.v1"),
            Vec::with_capacity(0), // Normally this would be a serialized `setup-monitoring.v1` model.
        );
        let output_event_data = output_event.encode_to_vec();
        sqlx::query!(
            "INSERT INTO in_table_pipelines (id, source, stage, event) VALUES ($1, $2, $3, $4);",
            &root_event.id,
            &root_event.source,
            &payload.stage,
            &output_event_data,
        )
        .execute(&mut tx)
        .await
        .context("error inserting row into in_table_pipelines")?;

        // Commit the transaction & return our output event.
        tx.commit().await.context("error committing database transaction")?;
        Ok(output_event)
    }
}
