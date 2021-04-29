//! Metadata controller.

mod cache;
mod storage;

use std::sync::Arc;

use anyhow::{bail, Context, Result};
use bytes::BytesMut;
use futures::stream::StreamExt;
use http::Method;
use prost::Message;
use sled::Tree;
use tokio::sync::{broadcast, mpsc, watch};
use tokio::task::JoinHandle;
use tokio_stream::wrappers::{BroadcastStream, ReceiverStream};

use crate::config::Config;
use crate::database::Database;
use crate::error::AppError;
use crate::models::{events, schema};
use crate::server::{must_get_token, require_method, send_error, send_response, H2Channel};
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
            proto::v1::ENDPOINT_METADATA_QUERY => handle_metadata_query(&self.config, &mut req).await,
            proto::v1::ENDPOINT_METADATA_SCHEMA_UPDATE => {
                handle_metadata_update_schema(
                    self.db.clone(),
                    self.tree.clone(),
                    &self.config,
                    self.cache.clone(),
                    &self.events_tx,
                    &mut req,
                    self.buf.clone(),
                )
                .await
            }
            _ => Err(AppError::ResourceNotFound.into()),
        };
        if let Err(err) = res {
            send_error(&mut req, self.buf.split(), err, std::convert::identity);
        }
    }
}

/// Handle a request to get the cluster's metadata.
#[tracing::instrument(level = "trace", skip(req))]
async fn handle_metadata_query(config: &Config, req: &mut H2Channel) -> Result<()> {
    let (ref mut req_chan, ref mut res_chan) = req;
    require_method(&req_chan, Method::GET)?;

    // TODO: require auth.
    let _creds = must_get_token(&req_chan, config)?;

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
#[tracing::instrument(level = "trace", skip(db, config, cache, events_tx, req, buf))]
async fn handle_metadata_update_schema(
    db: Database, tree: Tree, config: &Config, cache: Arc<MetadataCache>, events_tx: &broadcast::Sender<Arc<events::Event>>, req: &mut H2Channel,
    buf: BytesMut,
) -> Result<()> {
    let (ref mut req_chan, _res_chan) = req;
    require_method(&req_chan, Method::POST)?;
    let creds = must_get_token(&req_chan, config)?;
    let _claims = cache.must_get_token_claims(&creds.claims.id.as_u128())?;
    // TODO: verify any permission to modify schema, then pass along for static verification.
    // claims.check_schema_auth()

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
        if let Some(last_applied) = cache.get_schema_branch(branch) {
            if &last_applied >= timestamp {
                // If we've already applied this or a newer schema update timestamp for this
                // branch, then we respond with a no-op.
                let res = http::Response::builder()
                    .status(http::StatusCode::OK)
                    .header(http::header::CONTENT_TYPE, utils::HEADER_OCTET_STREAM)
                    .body(())
                    .context("error building response")?;
                let res_object = proto::v1::SchemaUpdateResponse { was_noop: true };
                send_response(req, buf, res, Some(res_object));
                return Ok(());
            }
        }
    }
    // Else, decode and validate the given statements & apply schema updates.
    let validated = schema::decode_and_validate(schema, cache.clone())?;
    let storage_events = storage::apply_schema_updates(db, tree, cache, validated, branch, timestamp).await?;
    for event in storage_events {
        let _ = events_tx.send(event);
    }

    // Respond with a success.
    let res = http::Response::builder()
        .status(http::StatusCode::OK)
        .header(http::header::CONTENT_TYPE, utils::HEADER_OCTET_STREAM)
        .body(())
        .context("error building response")?;
    let res_object = proto::v1::SchemaUpdateResponse { was_noop: false };
    send_response(req, buf, res, Some(res_object));
    Ok(())
}
