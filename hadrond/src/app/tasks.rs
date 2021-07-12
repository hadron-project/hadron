use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use arc_swap::ArcSwap;
use futures::stream::StreamExt;
use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tokio_stream::wrappers::{BroadcastStream, SignalStream};
use tokio_stream::StreamMap;
use tonic::transport::Server;

use crate::config::Config;
use crate::crd::Token;

/// A map of all known Token CRs in the namespace
pub type TokensMap = Arc<ArcSwap<HashMap<Arc<String>, Arc<Token>>>>;

/// Spawn a task which will watch all Token CRs in the namespace and update the returned map
/// according to the observed updates.
///
/// The returned value can be safely and quickly accessed from anywhere in the application.
pub(super) fn watch_tokens() -> TokensMap {
    todo!("impl this")
}
