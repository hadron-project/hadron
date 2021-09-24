//! Subscribe to a pipeline stage.

use std::sync::Arc;

use anyhow::{Context, Result};
use hadron::{Event, PipelineSubscribeResponse};
use structopt::StructOpt;

use crate::Hadron;

/// Subscribe to a pipeline stage.
#[derive(StructOpt)]
#[structopt(name = "sub")]
pub struct Sub {
    /// The pipeline to which the subscription should be made.
    pipeline: String,
    /// The pipeline stage to process.
    stage: String,
}

impl Sub {
    pub async fn run(&self, base: &Hadron) -> Result<()> {
        tracing::info!("subscribing to pipeline {} on stage {}", self.pipeline, self.stage);
        let handler = Arc::new(StdoutHandler { stage: self.stage.clone() });
        let client = base.get_client().await?;
        let sub = client
            .pipeline(&self.pipeline, &self.stage, handler)
            .await
            .context("error creating pipeline subscription")?;
        let _ = tokio::signal::ctrl_c().await;
        sub.cancel().await;
        Ok(())
    }
}

struct StdoutHandler {
    /// The pipeline stage being processed.
    stage: String,
}

#[hadron::async_trait]
impl hadron::PipelineHandler for StdoutHandler {
    #[tracing::instrument(level = "debug", skip(self, payload))]
    async fn handle(&self, payload: PipelineSubscribeResponse) -> Result<Event> {
        let mut data = vec![];
        let root_event = payload.root_event.unwrap_or_default();
        for (key, record) in payload.inputs.iter() {
            data.push((key, format!("{:?}", &record)));
        }
        data.sort_by(|a, b| a.0.cmp(b.0));
        tracing::info!(stage = ?payload.stage, root_event = ?root_event, inputs = ?data, "handling pipeline stage delivery");
        Ok(Event::new(root_event.id, self.stage.clone(), "empty".into(), Vec::with_capacity(0)))
    }
}
