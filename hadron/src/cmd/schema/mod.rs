//! Hadron schema management.

mod apply;
mod init;
mod new;

use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use structopt::StructOpt;

use crate::Hadron;

/// Hadron schema management.
#[derive(StructOpt)]
#[structopt(name = "schema")]
pub struct Schema {
    #[structopt(subcommand)]
    action: SchemaSubcommands,
}

impl Schema {
    pub async fn run(&self, base: &Hadron) -> Result<()> {
        match &self.action {
            SchemaSubcommands::Init(inner) => inner.run().await,
            SchemaSubcommands::New(inner) => inner.run().await,
            SchemaSubcommands::Apply(inner) => inner.run(base).await,
        }
    }
}

#[derive(StructOpt)]
enum SchemaSubcommands {
    /// Initialize a new schema management directory.
    Init(init::Init),
    /// Create a new schema changeset.
    New(new::New),
    /// Apply a schema update to the cluster.
    Apply(apply::Apply),
}

/// A model of the .hadron-state.yaml file of a managed schema directory.
#[derive(Serialize, Deserialize)]
pub struct HadronState {
    /// The branch name used for all schema changes managed in the corresponding directory.
    branch: String,
}

impl HadronState {
    pub const FILE_NAME: &'static str = ".hadron-state.yaml";

    pub async fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        let bytes = tokio::fs::read_to_string(&path).await.context("error reading .hadron-state.yaml file")?;
        let state = serde_yaml::from_str(bytes.as_str()).context("error deserializing contents of .hadron-state.yaml file")?;
        Ok(state)
    }
}
