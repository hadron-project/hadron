//! Publish data to a stream.

use anyhow::{Context, Result};
use structopt::StructOpt;

use crate::Hadron;

/// Publish data to a stream.
#[derive(StructOpt)]
#[structopt(name = "pub")]
pub struct Pub {
    /// The namespace/stream to which data should be published.
    #[structopt(short, long)]
    stream: String,
    /// The stream partition to which data should be published.
    #[structopt(short, long)]
    partition: String,
    /// The data payload to be published.
    data: String,
}

impl Pub {
    pub async fn run(&self, base: &Hadron) -> Result<()> {
        // Destructure the target namespace & stream.
        let mut splits = self.stream.splitn(2, '/');
        let (ns, stream) = (splits.next().unwrap_or(""), splits.next().unwrap_or(""));

        // Build a new client and submit the request to the cluster.
        tracing::info!("publishing data to {}", self.stream);
        let mut client = base.get_client().await?.publisher("hadron-cli", ns, stream);
        let res = client
            .publish_payload(From::from(self.data.clone()))
            .await
            .context("error publishing data")?;
        tracing::info!("Response: {:?}", res);
        Ok(())
    }
}
