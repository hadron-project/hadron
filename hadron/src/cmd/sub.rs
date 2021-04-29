//! Subscribe to data on a stream.

use std::sync::Arc;

use anyhow::{Context, Result};
use hadron::{StreamSubDelivery, SubscriberConfig, Subscription, SubscriptionStartingPoint};
use structopt::StructOpt;

use crate::Hadron;

/// Subscribe to data on a stream.
#[derive(StructOpt)]
#[structopt(name = "sub")]
pub struct Subscribe {
    /// The namespace/stream to which data should be published.
    #[structopt(short, long)]
    stream: String,
    /// The subscription group to use.
    #[structopt(short, long)]
    group: String,
}

impl Subscribe {
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
                durable: false,
                max_batch_size: 1,
                starting_point: SubscriptionStartingPoint::Beginning,
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
impl hadron::Handler for StdoutHandler {
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
