//! Subscribe to data on a stream.

use std::sync::Arc;

use anyhow::{Context, Result};
use hadron::{StreamSubscribeResponse, SubscriberConfig, SubscriptionStartingPoint};
use structopt::StructOpt;

use crate::Hadron;

/// Subscribe to data on a stream.
#[derive(StructOpt)]
#[structopt(name = "sub")]
pub struct Sub {
    /// The subscription group to use.
    #[structopt(short, long)]
    group: String,
    /// Make the new subscription durable.
    #[structopt(short, long)]
    durable: bool,
    /// The batch size to use for this subscription.
    #[structopt(short, long, default_value = "1")]
    batch_size: u32,
    /// Start from the first offset of the stream, defaults to latest.
    #[structopt(long, group = "start_point")]
    start_beginning: bool,
    /// Start from the latest offset of the stream, default.
    #[structopt(long, group = "start_point")]
    start_latest: bool,
    /// Start from the given offset, defaults to latest.
    #[structopt(long, group = "start_point")]
    start_offset: Option<u64>,
}

impl Sub {
    pub async fn run(&self, base: &Hadron) -> Result<()> {
        tracing::info!("subscribing to stream");
        let handler = Arc::new(StdoutHandler {});
        let config = Some(SubscriberConfig {
            durable: self.durable,
            max_batch_size: self.batch_size,
            starting_point: if let Some(offset) = self.start_offset {
                SubscriptionStartingPoint::Offset(offset)
            } else if self.start_beginning {
                SubscriptionStartingPoint::Beginning
            } else if self.start_latest {
                SubscriptionStartingPoint::Latest
            } else {
                SubscriptionStartingPoint::Latest
            },
        });
        let client = base.get_client().await?;
        let sub = client
            .subscribe(&self.group, config, handler)
            .await
            .context("error building subscription")?;
        let _ = tokio::signal::ctrl_c().await;
        sub.cancel().await;
        Ok(())
    }
}

struct StdoutHandler {}

#[hadron::async_trait]
impl hadron::StreamHandler for StdoutHandler {
    async fn handle(&self, payload: StreamSubscribeResponse) -> Result<()> {
        for record in payload.batch {
            let data = std::str::from_utf8(&record.data).unwrap_or("[binary data]");
            tracing::info!(
                id = %record.id,
                source = %record.source,
                specversion = %record.specversion,
                r#type = %record.r#type,
                optattrs = ?record.optattrs,
                data,
                "handling subscription delivery",
            );
        }
        Ok(())
    }
}
