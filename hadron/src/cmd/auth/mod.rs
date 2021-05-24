//! Hadron auth interaction.

mod token;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use structopt::StructOpt;

use crate::Hadron;

/// Hadron stream interaction.
#[derive(StructOpt)]
#[structopt(name = "auth")]
pub struct Auth {
    #[structopt(subcommand)]
    action: AuthSubcommands,
}

impl Auth {
    pub async fn run(&self, base: &Hadron) -> Result<()> {
        match &self.action {
            AuthSubcommands::CreateToken(inner) => inner.run(base).await,
        }
    }
}

#[derive(StructOpt)]
enum AuthSubcommands {
    /// Create a new auth token.
    CreateToken(token::CreateToken),
}
