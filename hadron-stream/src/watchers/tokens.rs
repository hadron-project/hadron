use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use arc_swap::ArcSwap;
use futures::stream::StreamExt;
use jsonwebtoken::DecodingKey;
use k8s_openapi::api::core::v1::Secret;
use kube::api::{Api, ListParams};
use kube::client::Client;
use kube::runtime::watcher::{watcher, Error as WatcherError, Event};
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tokio_stream::wrappers::BroadcastStream;

use crate::config::Config;
use hadron_core::crd::{RequiredMetadata, Token};

/// A map of all known Token CRs in the namespace.
pub type TokensMap = Arc<ArcSwap<HashMap<Arc<String>, Arc<Token>>>>;
/// A map of all known Secrets in the namespace belonging to Hadron.
pub type SecretsMap = Arc<ArcSwap<HashMap<Arc<String>, Arc<SecretWithDecodingKey>>>>;

/// A result type used for CR events coming from K8s.
pub type TokenCREventResult = std::result::Result<Event<Token>, WatcherError>;
/// A result type used for Secret events coming from K8s.
pub type SecretEventResult = std::result::Result<Event<Secret>, WatcherError>;

/// A K8s event watcher of Token CRs.
pub struct TokensWatcher {
    /// K8s client.
    client: Client,
    /// Runtime config.
    config: Arc<Config>,
    /// A channel used for triggering graceful shutdown.
    shutdown: BroadcastStream<()>,

    tokens: TokensMap,
    secrets: SecretsMap,
}

impl TokensWatcher {
    /// Create a new instance.
    pub fn new(client: Client, config: Arc<Config>, shutdown: broadcast::Receiver<()>) -> (Self, TokensMap, SecretsMap) {
        let shutdown = BroadcastStream::new(shutdown);
        let tokens: TokensMap = Default::default();
        let secrets: SecretsMap = Default::default();
        (
            Self {
                client,
                config,
                shutdown,
                tokens: tokens.clone(),
                secrets: secrets.clone(),
            },
            tokens,
            secrets,
        )
    }

    pub fn spawn(self) -> JoinHandle<Result<()>> {
        tokio::spawn(self.run())
    }

    async fn run(mut self) -> Result<()> {
        let params = list_params_cluster_selector_labels();
        let tokens_api: Api<Token> = Api::namespaced(self.client.clone(), &self.config.namespace);
        let tokens_watcher = watcher(tokens_api, ListParams::default());
        let secrets_api: Api<Secret> = Api::namespaced(self.client.clone(), &self.config.namespace);
        let secrets_watcher = watcher(secrets_api, params.clone());
        tokio::pin!(tokens_watcher, secrets_watcher);

        tracing::info!("Tokens CR watcher initialized");
        loop {
            tokio::select! {
                Some(k8s_event_res) = tokens_watcher.next() => self.handle_token_event(k8s_event_res).await,
                Some(k8s_event_res) = secrets_watcher.next() => self.handle_secret_event(k8s_event_res).await,
                _ = self.shutdown.next() => break,
            }
        }

        Ok(())
    }

    /// Handle Token CR watcher events coming from K8s.
    #[tracing::instrument(level = "debug", skip(self, res))]
    async fn handle_token_event(&mut self, res: TokenCREventResult) {
        let event = match res {
            Ok(event) => event,
            Err(err) => {
                tracing::error!(error = ?err, "error from k8s Token CR watcher stream");
                let _ = tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                return;
            }
        };
        match event {
            Event::Applied(token) => {
                let name = match token.metadata.name.as_ref() {
                    Some(name) => name,
                    None => return,
                };
                tracing::debug!(%name, "indexing Token CR");
                let orig = self.tokens.load_full();
                let mut updated = orig.as_ref().clone();
                updated.insert(Arc::new(name.to_string()), Arc::new(token));
                self.tokens.store(Arc::new(updated));
            }
            Event::Deleted(token) => {
                let name = match token.metadata.name.as_ref() {
                    Some(name) => name,
                    None => return,
                };
                tracing::debug!(%name, "deleting Token CR");
                let orig = self.tokens.load_full();
                let mut updated = orig.as_ref().clone();
                if let Some(_old) = updated.remove(name) {
                    tracing::debug!(%name, "old Token CR found and deleted from index");
                }
                self.tokens.store(Arc::new(updated));
            }
            Event::Restarted(tokens) => {
                tracing::debug!("Tokens CR watcher stream restarted");
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

    /// Handle Secret watcher events coming from K8s.
    #[tracing::instrument(level = "debug", skip(self, res))]
    async fn handle_secret_event(&mut self, res: SecretEventResult) {
        let event = match res {
            Ok(event) => event,
            Err(err) => {
                tracing::error!(error = ?err, "error from k8s Secret watcher stream");
                let _ = tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                return;
            }
        };
        match event {
            Event::Applied(secret) => {
                let name = match secret.metadata.name.as_ref() {
                    Some(name) => name,
                    None => return,
                };
                tracing::debug!(%name, "indexing secret");
                let decoding_key = match extract_decoding_key_from_secret(&secret) {
                    Ok(decoding_key) => decoding_key,
                    Err(err) => {
                        tracing::error!(error = ?err, "error extracting hmac decoding key from secret");
                        return;
                    }
                };
                let orig = self.secrets.load_full();
                let mut updated = orig.as_ref().clone();
                updated.insert(Arc::new(name.to_string()), Arc::new(SecretWithDecodingKey::new(secret, decoding_key)));
                self.secrets.store(Arc::new(updated));
            }
            Event::Deleted(secret) => {
                let name = match secret.metadata.name.as_ref() {
                    Some(name) => name,
                    None => return,
                };
                tracing::debug!(%name, "deleting Secret from index");
                let orig = self.secrets.load_full();
                let mut updated = orig.as_ref().clone();
                if let Some(_old) = updated.remove(name) {
                    tracing::debug!(%name, "old Secret found and deleted from index");
                }
                self.secrets.store(Arc::new(updated));
            }
            Event::Restarted(secrets) => {
                tracing::debug!("Secret watcher stream restarted");
                let new_tokens = secrets.into_iter().fold(HashMap::new(), |mut acc, secret| {
                    let key = match secret.metadata.name.as_ref() {
                        Some(name) => Arc::new(name.clone()),
                        None => return acc,
                    };
                    let decoding_key = match extract_decoding_key_from_secret(&secret) {
                        Ok(decoding_key) => decoding_key,
                        Err(err) => {
                            tracing::error!(error = ?err, "error extracting hmac decoding key from secret");
                            return acc;
                        }
                    };
                    let val = Arc::new(SecretWithDecodingKey::new(secret, decoding_key));
                    acc.insert(key, val);
                    acc
                });
                tracing::debug!(len = new_tokens.len(), "new Secrets map created");
                self.secrets.store(Arc::new(new_tokens));
            }
        }
    }
}

/// A K8s Secret along with its associated decoding key.
pub struct SecretWithDecodingKey {
    _secret: Secret,
    key: DecodingKey<'static>,
}

impl SecretWithDecodingKey {
    /// Construct a new instance.
    pub fn new(_secret: Secret, key: DecodingKey<'static>) -> Self {
        Self { _secret, key }
    }

    /// Get a reference to the Secret's decoding key.
    pub fn key(&self) -> &DecodingKey<'static> {
        &self.key
    }
}

/// Extract the decoding key from the given Secret.
fn extract_decoding_key_from_secret(secret: &Secret) -> Result<DecodingKey<'static>> {
    let data = secret.data.as_ref().context("no data found in secret")?;
    let hmac_key = data.get(hadron_core::auth::SECRET_HMAC_KEY).context("no secret data found for HMAC key")?;
    Ok(DecodingKey::from_secret(&hmac_key.0).into_static())
}

/// Create a list params object which selects only objects which matching Hadron labels.
fn list_params_cluster_selector_labels() -> ListParams {
    ListParams {
        label_selector: Some(hadron_core::HADRON_OPERATOR_LABEL_SELECTORS.into()),
        ..Default::default()
    }
}
