//! The Hadron CLI.

mod client;
mod cmd;

use anyhow::Result;
use hadron::Client;
use structopt::StructOpt;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::prelude::*;
use tracing_subscriber::{fmt, EnvFilter};

use client::new_client;

/// The Hadron CLI.
#[derive(StructOpt)]
#[structopt(name = "hadron")]
pub struct Hadron {
    #[structopt(subcommand)]
    action: HadronSubcommands,
    /// Enable debug logging.
    #[structopt(short)]
    verbose: bool,
    /// Set the URL of the cluster to interact with.
    #[structopt(long)]
    url: Option<String>,
    /// Set the auth token to use for interacting with the cluster.
    #[structopt(long, conflicts_with("username"), conflicts_with("password"))]
    token: Option<String>,
    // /// Set the username to use for interacting with the cluster.
    // #[structopt(long, requires("password"), conflicts_with("token"))]
    // username: Option<String>,
    // /// Set the password to use for interacting with the cluster.
    // #[structopt(long, requires("username"), conflicts_with("token"))]
    // password: Option<String>,
}

impl Hadron {
    pub async fn run(self) -> Result<()> {
        // Initialize logging based on CLI config.
        let fmt_layer = fmt::layer().with_target(true);
        let filter_layer;
        let level_filter;
        if self.verbose {
            filter_layer = EnvFilter::new("debug");
            level_filter = LevelFilter::DEBUG;
        } else {
            filter_layer = EnvFilter::new("info");
            level_filter = LevelFilter::INFO;
        }
        tracing_subscriber::registry()
            .with(filter_layer)
            .with(fmt_layer)
            .with(level_filter)
            .init();

        match &self.action {
            HadronSubcommands::Pipeline(inner) => inner.run(&self).await,
            HadronSubcommands::Stream(inner) => inner.run(&self).await,
        }
    }

    /// Get a new client instance based on runtime config.
    pub async fn get_client(&self) -> Result<Client> {
        new_client(
            self.url.as_deref(),
            self.token.as_deref(),
            None, // self.username.as_deref(),
            None, // self.password.as_deref(),
        )
        .await
    }
}

#[derive(StructOpt)]
pub enum HadronSubcommands {
    /// Hadron pipeline interaction.
    #[structopt(name = "pipeline")]
    Pipeline(cmd::pipeline::Pipeline),
    /// Hadron stream interaction.
    #[structopt(name = "stream")]
    Stream(cmd::stream::Stream),
}
