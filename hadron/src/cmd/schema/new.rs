//! Schema changeset creation.

#![allow(dead_code)] // TODO: remove

use anyhow::Result;
use structopt::StructOpt;

/// Create a new schema changeset.
#[derive(StructOpt)]
#[structopt(name = "new")]
pub struct New {
    /// The directory in which to create the new Hadron schema changeset.
    #[structopt(short, long, default_value = "hadron-schema")]
    dir: String,
    /// The name of the branch to use for the new changeset; will use the branch name of the
    /// latest changeset file in the target schema management dir.
    #[structopt(short, long)]
    branch_name: String,
}

impl New {
    pub async fn run(&self) -> Result<()> {
        Ok(())
    }
}
