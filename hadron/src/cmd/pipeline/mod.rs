//! Hadron pipeline interaction.

pub mod sub;

use anyhow::Result;
use structopt::StructOpt;

use crate::Hadron;

/// Hadron stream interaction.
#[derive(StructOpt)]
#[structopt(name = "pipeline")]
pub struct Pipeline {
    #[structopt(subcommand)]
    action: PipelineSubcommands,
}

impl Pipeline {
    pub async fn run(&self, base: &Hadron) -> Result<()> {
        match &self.action {
            PipelineSubcommands::Sub(inner) => inner.run(base).await,
        }
    }
}

#[derive(StructOpt)]
enum PipelineSubcommands {
    /// Subscribe to data on a stream.
    Sub(sub::Sub),
}
