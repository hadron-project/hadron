use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use arc_swap::ArcSwap;
use futures::stream::StreamExt;
use kube::api::{Api, ListParams};
use kube::client::Client;
use kube_runtime::watcher::{watcher, Error as WatcherError, Event};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tokio_stream::wrappers::BroadcastStream;

use crate::config::Config;
use hadron_core::crd::{RequiredMetadata, Token};

/// A map of all known Token CRs in the namespace.
pub type TokensMap = Arc<ArcSwap<HashMap<Arc<String>, Arc<Token>>>>;

/// A result type used for CR events coming from K8s.
pub type TokenCREventResult = std::result::Result<Event<Token>, WatcherError>;

/// A K8s event watcher of Token CRs.
pub struct TokensWatcher {
    /// K8s client.
    client: Client,
    /// Runtime config.
    _config: Arc<Config>,
    /// A channel used for triggering graceful shutdown.
    shutdown: BroadcastStream<()>,

    tokens: TokensMap,
}

impl TokensWatcher {
    /// Create a new instance.
    pub fn new(client: Client, _config: Arc<Config>, shutdown: broadcast::Receiver<()>) -> (Self, TokensMap) {
        let shutdown = BroadcastStream::new(shutdown);
        let tokens: TokensMap = Default::default();
        (Self { client, _config, shutdown, tokens: tokens.clone() }, tokens)
    }

    pub fn spawn(self) -> JoinHandle<Result<()>> {
        tokio::spawn(self.run())
    }

    async fn run(mut self) -> Result<()> {
        let tokens_api: Api<Token> = Api::all(self.client.clone());
        let stream = watcher(tokens_api, ListParams::default());
        tokio::pin!(stream);

        tracing::info!("Tokens CR watcher initialized");
        loop {
            tokio::select! {
                Some(k8s_event_res) = stream.next() => self.handle_k8s_event(k8s_event_res).await,
                _ = self.shutdown.next() => break,
            }
        }

        Ok(())
    }

    /// Handle watcher events coming from K8s.
    #[tracing::instrument(level = "debug", skip(self, res))]
    async fn handle_k8s_event(&mut self, res: TokenCREventResult) {
        let event = match res {
            Ok(event) => event,
            Err(err) => {
                tracing::error!(error = ?err, "error from k8s watch stream");
                let _ = tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                return;
            }
        };
        match event {
            Event::Applied(token) => {
                let name = match &token.metadata.name {
                    Some(name) => name,
                    None => return,
                };
                tracing::debug!(%name, "adding new Token CR");
                let orig = self.tokens.load_full();
                let mut updated = orig.as_ref().clone();
                updated.insert(Arc::new(name.to_string()), Arc::new(token));
                self.tokens.store(Arc::new(updated));
            }
            Event::Deleted(token) => {
                let name = match &token.metadata.name {
                    Some(name) => name,
                    None => return,
                };
                tracing::debug!(%name, "deleting Token CR");
                let orig = self.tokens.load_full();
                let mut updated = orig.as_ref().clone();
                if let Some(_old) = updated.remove(name) {
                    tracing::debug!(%name, "old Token CR found and deleted");
                }
                self.tokens.store(Arc::new(updated));
            }
            Event::Restarted(tokens) => {
                tracing::debug!("Tokens CR stream restarted");
                let new_tokens = tokens.into_iter().fold(HashMap::new(), |mut acc, token| {
                    let key = Arc::new(token.name().to_string());
                    let val = Arc::new(token);
                    acc.insert(key, val);
                    acc
                });
                tracing::debug!(len = new_tokens.len(), "new Tokens CR map created");
                self.tokens.store(Arc::new(new_tokens));
            }
        }
    }
}
