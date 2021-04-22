//! Publish data to a stream.

#![allow(dead_code)] // TODO: remove

use anyhow::{Context, Result};
use structopt::StructOpt;

use crate::Hadron;

/// Apply schema changes to the cluster.
#[derive(StructOpt)]
#[structopt(name = "publish")]
pub struct Publish {
    /// The namespace of the stream to which data should be published.
    #[structopt(short, long)]
    ns: String,
    /// The stream to which data should be published.
    #[structopt(short, long)]
    stream: String,
    /// The stream partition to which data should be published.
    #[structopt(short, long)]
    partition: String,
    /// The data payload to be published.
    data: String,
}

impl Publish {
    pub async fn run(&self, base: &Hadron) -> Result<()> {
        println!("about to publish some data");
        // Build a new client and submit the request to the cluster.
        let mut client = base.get_client().await?.publisher("hadron-cli");
        let res = client
            .publish_payload(&self.ns, &self.stream, &self.partition, From::from(self.data.clone()))
            .await
            .context("error publishing data")?;
        println!("Response:\n{:?}", res);
        Ok(())
    }
}
