//! Subscribe to data on a stream.

use std::sync::Arc;

use anyhow::{Context, Result};
use hadron::{StreamSubDelivery, SubscriberConfig, Subscription, SubscriptionStartingPoint};
use pest::Parser;
use structopt::StructOpt;

use crate::parser::{GrantsParser, Rule};
use crate::Hadron;

/// Create a new auth token.
#[derive(StructOpt)]
#[structopt(name = "create-token")]
pub struct CreateToken {
    /// Grant access to all resources of the cluster.
    #[structopt(long, group = "access")]
    all: bool,
    /// Grant access to only the metrics resources of the cluster.
    #[structopt(long, group = "access")]
    metrics: bool,
    /// Namespaced grants for the new token.
    #[structopt(long, group = "access")]
    grants: Option<Vec<String>>,
}

impl CreateToken {
    pub async fn run(&self, base: &Hadron) -> Result<()> {
        // Parse the given grants and transform them into API compatible objects.
        let grants = match self.grants.as_ref() {
            Some(grants) => Some(GrantsParser::parse_grants(grants)?),
            None => None,
        };

        let client = base.get_client().await?.auth();
        let token = client.create_token(self.all, self.metrics, grants).await?;
        tracing::info!(?token, "new token created successfully");
        Ok(())
    }
}
