//! Subscribe to data on a stream.

use std::sync::Arc;

use anyhow::{Context, Result};
use hadron::{StreamSubDelivery, SubscriberConfig, Subscription, SubscriptionStartingPoint};
use structopt::StructOpt;

use crate::Hadron;

/// Subscribe to data on a stream.
#[derive(StructOpt)]
#[structopt(name = "sub")]
pub struct Sub {
    /// The namespace/stream to which the subscription should be made.
    #[structopt(short, long)]
    stream: String,
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
        // Destructure the target namespace & stream.
        let mut splits = self.stream.splitn(2, '/');
        let (ns, stream) = (splits.next().unwrap_or(""), splits.next().unwrap_or(""));

        tracing::info!("subscribing to stream {}/{}", ns, stream);
        let handler = Arc::new(StdoutHandler {});
        let client = base.get_client().await?;
        let sub = client.subscribe(
            handler,
            ns,
            stream,
            &self.group,
            Some(SubscriberConfig {
                durable: self.durable,
                max_batch_size: self.batch_size,
                starting_point: if let Some(offset) = self.start_offset {
                    SubscriptionStartingPoint::Offset(offset)
                } else if self.start_beginning {
                    SubscriptionStartingPoint::Beginning
                } else {
                    SubscriptionStartingPoint::Latest
                },
            }),
        );
        let _ = tokio::signal::ctrl_c().await;
        sub.cancel().await;
        Ok(())
    }

    /// Handle a subscription payload delivery.
    async fn handle_delivery_payload() -> Result<()> {
        Ok(())
    }
}

struct StdoutHandler {}

#[hadron::async_trait]
impl hadron::StreamHandler for StdoutHandler {
    async fn handle(&self, payload: StreamSubDelivery) -> Result<()> {
        for record in payload.batch {
            match std::str::from_utf8(&record.data) {
                Ok(strdata) => tracing::info!(offset = record.offset, data = strdata, "handling subscription delivery"),
                Err(_) => tracing::info!(offset = record.offset, data = "[binary data]", "handling subscription delivery"),
            }
        }
        Ok(())
    }
}
