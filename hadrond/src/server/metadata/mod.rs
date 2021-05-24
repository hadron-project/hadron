//! Metadata controller.

mod cache;
mod storage;

use std::sync::Arc;

use anyhow::{bail, Context, Result};
use bytes::BytesMut;
use futures::stream::StreamExt;
use http::Method;
use jsonwebtoken::{encode as encode_jwt, Algorithm, Header};
use prost::Message;
use sled::Tree;
use tokio::sync::{broadcast, mpsc, watch};
use tokio::task::JoinHandle;
use tokio_stream::wrappers::{BroadcastStream, ReceiverStream};

use crate::auth::{Claims, UserRole};
use crate::config::{Config, MetadataConfig};
use crate::database::Database;
use crate::error::{AppError, ShutdownError};
use crate::models::{events, schema};
use crate::server::{must_get_token, must_get_user, require_method, send_error, send_response, H2Channel};
use crate::utils;

pub use cache::MetadataCache;

/// Network server used to handle client requests.
pub struct MetadataCtl {
    /// The application's runtime config.
    config: Arc<Config>,
    /// The application's database system.
    db: Database,
    /// The metadata DB tree.
    tree: Tree,
    /// The system metadata cache.
    cache: Arc<MetadataCache>,
    /// A channel used for triggering graceful shutdown.
    shutdown_tx: broadcast::Sender<()>,
    /// A channel used for triggering graceful shutdown.
    shutdown_rx: BroadcastStream<()>,

    /// The channel used to handle inbound metadata requests.
    requests: ReceiverStream<H2Channel>,

    /// A channel used for emiting events from this controller.
    events_tx: broadcast::Sender<Arc<events::Event>>,

    /// A general purpose reusable bytes buffer, safe for concurrent use.
    buf: BytesMut,
}

impl MetadataCtl {
    /// Create a new instance.
    pub async fn new(
        config: Arc<Config>, db: Database, shutdown_tx: broadcast::Sender<()>, requests: mpsc::Receiver<H2Channel>,
        events_tx: broadcast::Sender<Arc<events::Event>>,
    ) -> Result<(Self, Arc<MetadataCache>)> {
        let shutdown_rx = BroadcastStream::new(shutdown_tx.subscribe());

        // Recover system state & expose metadata cache.
        let tree = db.get_metadata_tree().await.context("error opening metadata database tree")?;
        let (cache, init_event) = storage::recover_system_state(&tree).await.context("error recovering system state")?;
        let cache = Arc::new(cache);
        let _ = events_tx.send(Arc::new(events::Event::Initial(init_event)));

        Ok((
            Self {
                config,
                db,
                tree,
                cache: cache.clone(),
                shutdown_tx,
                shutdown_rx,
                requests: ReceiverStream::new(requests),
                events_tx,
                buf: BytesMut::with_capacity(5000),
            },
            cache,
        ))
    }

    pub fn spawn(self) -> JoinHandle<Result<()>> {
        tokio::spawn(self.run())
    }

    async fn run(mut self) -> Result<()> {
        tracing::debug!("metadata controller started");

        loop {
            tokio::select! {
                Some(req) = self.requests.next() => self.handle_request(req).await,
                Some(needs_shutdown) = self.shutdown_rx.next() => break,
            }
        }

        tracing::debug!("metadata controller shutdown");
        Ok(())
    }

    /// Handle a request which has been sent to this controller.
    #[tracing::instrument(level = "trace", skip(self, req))]
    async fn handle_request(&mut self, mut req: H2Channel) {
        let path = req.0.uri().path();
        let res = match path {
            proto::v1::ENDPOINT_METADATA_QUERY => self.handle_metadata_query(&mut req).await,
            proto::v1::ENDPOINT_METADATA_SCHEMA_UPDATE => self.handle_metadata_update_schema(&mut req).await,
            proto::v1::ENDPOINT_METADATA_AUTH_CREATE_TOKEN => self.handle_create_token(&mut req).await,
            _ => Err(AppError::ResourceNotFound.into()),
        };
        if let Err(err) = res {
            tracing::error!(error = ?err, "error handling metadata request");
            if err.downcast_ref::<ShutdownError>().is_some() {
                let _ = self.shutdown_tx.send(());
            }
            send_error(&mut req, self.buf.split(), err, std::convert::identity);
        }
    }

    /// Handle a request to create a new auth token.
    ///
    /// This endpoint can only be called by a user, and the calling user must be either a
    /// root or admin user.
    ///
    /// A successful token creation will result in a new token being created and stored on disk — only
    /// the token ID and its permissions model are stored on disk.
    #[tracing::instrument(level = "trace", skip(self, req))]
    async fn handle_create_token(&mut self, req: &mut H2Channel) -> Result<()> {
        let (ref mut req_chan, ref mut res_chan) = req;
        require_method(&req_chan, Method::POST)?;

        // Extract the calling user's info and then fetch their data from metadata cache.
        let creds = must_get_user(&req_chan)?;
        let user = creds.username()?;
        let pass = creds.password()?;
        let user = self.cache.must_get_user(user, pass)?;
        if !matches!(user.role(), UserRole::Root | UserRole::Admin) {
            bail!(AppError::Unauthorized);
        }

        // Extract and validate contents of request.
        let body_stream = req_chan.body_mut();
        let body = body_stream
            .data()
            .await
            .context("no body found in request")?
            .context("error reading request body")?;

        // Decode body & validate the contents of the request.
        let token_req = proto::v1::CreateTokenRequest::decode(body.as_ref()).map_err(|err| AppError::InvalidInput(err.to_string()))?;
        let claims = Claims::from_create_token_request(token_req)?;
        let jwt =
            encode_jwt(&Header::new(Algorithm::RS512), &claims, &self.config.metadata_config.jwt_encoding_key).context("error encoding new JWT")?;

        // Apply the new token to disk for long-term usage & respond with new token info.
        storage::create_token(self.tree.clone(), &claims).await?;
        let res = http::Response::builder()
            .status(http::StatusCode::OK)
            .header(http::header::CONTENT_TYPE, utils::HEADER_OCTET_STREAM)
            .body(())
            .context("error building response")?;
        let res_object = proto::v1::CreateTokenResponse {
            result: Some(proto::v1::CreateTokenResponseResult::Ok(jwt)),
        };
        send_response(req, self.buf.split(), res, Some(res_object));
        Ok(())
    }

    /// Handle a request to get the cluster's metadata.
    #[tracing::instrument(level = "trace", skip(self, req))]
    async fn handle_metadata_query(&self, req: &mut H2Channel) -> Result<()> {
        let (ref mut req_chan, ref mut res_chan) = req;
        require_method(&req_chan, Method::GET)?;

        let creds = must_get_token(&req_chan, &self.config)?;
        let _claims = self.cache.must_get_token_claims(&creds.claims.id.as_u128())?;

        // Stub: respond with some random yaml.
        let body = bytes::Bytes::from(r#"{"nothing":"so far"}"#);
        let mut response = http::Response::new(());
        response
            .headers_mut()
            .append("content-type", "application/json".parse().context("could not parse header for response")?);
        let mut body_res = res_chan.send_response(response, false).context("error sending header response")?;
        body_res.send_data(body, true).context("error sending body response")?;
        Ok(())
    }

    /// Handle a request to update the clsuter's schema.
    #[tracing::instrument(level = "trace", skip(self, req))]
    async fn handle_metadata_update_schema(&mut self, req: &mut H2Channel) -> Result<()> {
        let (ref mut req_chan, _res_chan) = req;
        require_method(&req_chan, Method::POST)?;
        let creds = must_get_token(&req_chan, &self.config)?;
        let claims = self.cache.must_get_token_claims(&creds.claims.id.as_u128())?;

        // Extract and validate contents of request.
        let body_stream = req_chan.body_mut();
        let body = body_stream
            .data()
            .await
            .context("no body found in request")?
            .context("error reading request body")?;

        // Decode body and check to see if this schema update has already been applied.
        let update_req = proto::v1::SchemaUpdateRequest::decode(body.as_ref()).map_err(|err| AppError::InvalidInput(err.to_string()))?;
        let (schema, branch, timestamp) = match &update_req.r#type {
            Some(proto::v1::SchemaUpdateRequestType::Managed(inner)) => (&inner.schema, Some(&inner.branch), Some(&inner.timestamp)),
            Some(proto::v1::SchemaUpdateRequestType::Oneoff(inner)) => (&inner.schema, None, None),
            None => bail!(AppError::InvalidInput("schema update request did not contain a valid update type".into())),
        };
        if let (Some(branch), Some(timestamp)) = (branch, timestamp) {
            if let Some(last_applied) = self.cache.get_schema_branch(branch) {
                if &last_applied >= timestamp {
                    // If we've already applied this or a newer schema update timestamp for this
                    // branch, then we respond with a no-op.
                    let res = http::Response::builder()
                        .status(http::StatusCode::OK)
                        .header(http::header::CONTENT_TYPE, utils::HEADER_OCTET_STREAM)
                        .body(())
                        .context("error building response")?;
                    let res_object = proto::v1::SchemaUpdateResponse { was_noop: true };
                    send_response(req, self.buf.split(), res, Some(res_object));
                    return Ok(());
                }
            }
        }
        // Else, decode and validate the given statements & apply schema updates.
        let validated = schema::decode_and_validate(schema, &self.cache, &claims)?;
        let storage_events =
            storage::apply_schema_updates(self.db.clone(), self.tree.clone(), self.cache.clone(), validated, branch, timestamp).await?;
        for event in storage_events {
            let _ = self.events_tx.send(event);
        }

        // Respond with a success.
        let res = http::Response::builder()
            .status(http::StatusCode::OK)
            .header(http::header::CONTENT_TYPE, utils::HEADER_OCTET_STREAM)
            .body(())
            .context("error building response")?;
        let res_object = proto::v1::SchemaUpdateResponse { was_noop: false };
        send_response(req, self.buf.split(), res, Some(res_object));
        Ok(())
    }
}
