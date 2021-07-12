//! Hadron stream interaction.

pub mod r#pub;
pub mod sub;

use anyhow::Result;
use structopt::StructOpt;

use crate::Hadron;

/// Hadron stream interaction.
#[derive(StructOpt)]
#[structopt(name = "stream")]
pub struct Stream {
    #[structopt(subcommand)]
    action: StreamSubcommands,
}

impl Stream {
    pub async fn run(&self, base: &Hadron) -> Result<()> {
        match &self.action {
            StreamSubcommands::Pub(inner) => inner.run(base).await,
            StreamSubcommands::Sub(inner) => inner.run(base).await,
        }
    }
}

#[derive(StructOpt)]
enum StreamSubcommands {
    /// Publish data to a stream.
    Pub(r#pub::Pub),
    /// Subscribe to data on a stream.
    Sub(sub::Sub),
}
