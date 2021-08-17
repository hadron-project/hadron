//! The Hadron CLI.

use anyhow::Result;
use structopt::StructOpt;

use hadron_cli::Hadron;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let cli = Hadron::from_args();
    cli.run().await
}
