//! Metadata cache.

use std::sync::Arc;

use dashmap::DashMap;

use crate::crd::Token;
use crate::error::AppError;

/// A cache of cluster metadata.
///
/// It is important to note that we must reduce lock contention as much as possible. This can
/// easily be achieved by simply dropping the lock guard as soon as possible whenever a lock is
/// taken, especially given that all internal data is Arc'd and can be easily cloned, allowing the
/// lock to be dropped very quickly.
#[derive(Default)]
pub struct MetadataCache {
    /// Index of tokens.
    tokens: DashMap<String, Arc<Token>>,
}

impl MetadataCache {
    /// Insert a token into the metadata cache.
    ///
    /// This will only insert the token into the cache if the token has been assigned an ID, which
    /// indicates that the token has been created in K8s.
    pub fn insert_token(&self, token: Arc<Token>) {
        if let Some(status) = &token.status {
            self.tokens.insert(status.token_id.clone(), token);
        }
    }

    /// Remove the token from the cache.
    pub fn remove_token(&self, token: Arc<Token>) {
        if let Some(status) = &token.status {
            self.tokens.remove(&status.token_id);
        }
    }

    /// Get the given token's claims, else return an auth error.
    pub fn must_get_token_claims(&self, token_id: &str) -> anyhow::Result<Arc<Token>> {
        match self.tokens.get(token_id).map(|val| val.value().clone()) {
            Some(claims) => Ok(claims),
            None => Err(AppError::UnknownToken.into()),
        }
    }
}
