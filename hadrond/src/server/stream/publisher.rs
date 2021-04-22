use std::pin::Pin;
use std::task::Poll;

use anyhow::{bail, Context, Result};
use bytes::Bytes;
use futures::prelude::*;
use http::Method;
use prost::Message;
use proto::v1::{
    StreamPubRequest, StreamPubResponse, StreamPubResponseOk, StreamPubResponseResult, StreamPubSetupRequest, StreamPubSetupResponse,
    StreamPubSetupResponseResult,
};

use crate::database::Database;
use crate::error::{AppError, ShutdownError, ERR_DB_FLUSH};
use crate::server::stream::{subscriber::StreamSubCtlMsg, StreamCtl};
use crate::server::{must_get_token, require_method, H2Channel, H2DataChannel};
use crate::utils;

impl StreamCtl {
    /// Validate a stream publisher channel before full setup.
    #[tracing::instrument(level = "trace", skip(self, req))]
    pub(super) async fn validate_publisher_channel(&mut self, req: &mut H2Channel) -> Result<h2::SendStream<Bytes>> {
        require_method(&req.0, Method::POST)?;
        let creds = must_get_token(&req.0, self.config.as_ref())?;
        let claims = self.cache.must_get_token_claims(&creds.claims.id.as_u128())?;
        claims.check_stream_pub_auth(&self.stream.metadata.namespace, &self.stream.metadata.name)?;

        // Read initial body so that we know the name of the publisher.
        let mut body_req = req
            .0
            .body_mut()
            .data()
            .await
            .context("no body received while setting up publisher channel")?
            .context("error awaiting request body while setting up publisher channel")?;
        let body = StreamPubSetupRequest::decode(&mut body_req).context("error decoding stream pub setup request")?;
        tracing::debug!("finished validating publisher setup for publisher: {}", body.name);

        // Respond to publisher to let them know that the channel is ready for use.
        let mut res = http::Response::new(());
        *res.status_mut() = http::StatusCode::OK;
        let mut res_chan = req
            .1
            .send_response(res, false)
            .context("error returning response to publisher for channel setup")?;
        let setup_res = StreamPubSetupResponse {
            result: Some(StreamPubSetupResponseResult::Ok(Default::default())),
        };
        let mut setup_res_buf = self.buf.split();
        setup_res.encode(&mut setup_res_buf).context("error encoding stream pub setup response")?;
        res_chan
            .send_data(setup_res_buf.freeze(), false)
            .context("error sending stream pub setup response")?;
        Ok(res_chan)
    }

    #[tracing::instrument(level = "trace", skip(self, res))]
    pub(super) async fn handle_publisher_request(&mut self, res: Result<(H2DataChannel, Bytes)>) {
        // Unpack data payload and channel.
        let (mut chan, data) = match res {
            Ok(chan_and_data) => chan_and_data,
            Err(err) => {
                tracing::error!(error = ?err, "error awaiting data from publisher");
                return;
            }
        };
        // Publish the new data frame.
        let mut buf = self.buf.split();
        match self.publish_data_frame(&mut chan, data).await {
            Ok(last_offset) => {
                let proto = StreamPubResponse {
                    result: Some(StreamPubResponseResult::Ok(StreamPubResponseOk { last_offset })),
                };
                if let Err(err) = proto.encode(&mut buf) {
                    tracing::error!(error = ?err, "error encoding publisher response payload");
                }
            }
            Err(err) => {
                tracing::error!(error = ?err, "error while publishing data to stream");
                if err.downcast_ref::<ShutdownError>().is_some() {
                    let _ = self.shutdown_tx.send(());
                }
                let app_err = err.downcast::<AppError>().unwrap_or_else(AppError::from);
                let proto = StreamPubResponse {
                    result: Some(StreamPubResponseResult::Err(proto::v1::Error {
                        message: app_err.to_string(),
                    })),
                };
                if let Err(err) = proto.encode(&mut buf) {
                    tracing::error!(error = ?err, "error encoding publisher response payload");
                }
            }
        };
        if let Err(err) = chan.1.send_data(buf.freeze(), false) {
            tracing::error!(error = ?err, "error sending resposne to publisher client");
        }
        self.publishers.push(PublisherFut::new(chan));
    }

    /// Publish a frame of data to the target stream, returning the offset of the last entry written.
    #[tracing::instrument(level = "trace", skip(self, _chan, data))]
    async fn publish_data_frame(&mut self, _chan: &mut H2DataChannel, mut data: Bytes) -> Result<u64> {
        tracing::debug!(self.next_offset, "writing data to stream");

        // Decode the payload.
        let data = StreamPubRequest::decode(&mut data).map_err(|err| AppError::InvalidInput(err.to_string()))?;
        if data.batch.is_empty() {
            bail!(AppError::InvalidInput("entries batch was empty, no-op".into()));
        }

        // Assign an offset to each entry in the payload and write as a batch.
        let mut batch = sled::Batch::default();
        for entry in data.batch {
            tracing::debug!("data: {}", String::from_utf8_lossy(&entry));
            batch.insert(&utils::encode_u64(self.next_offset), entry);
            self.next_offset += 1;
        }
        let tree = self.tree.clone();
        Database::spawn_blocking(move || {
            tree.apply_batch(batch)
                .context("error applying write batch")
                .map_err(ShutdownError::from)?;
            tree.flush().context(ERR_DB_FLUSH).map_err(ShutdownError::from)
        })
        .await??;

        // FUTURE: Send the channel to a watch group to await async replication.

        // Respond to publisher.
        tracing::debug!(self.next_offset, "finished writing data to stream");
        let _ = self.subs_tx.send(StreamSubCtlMsg::NextOffsetUpdated(self.next_offset)).await;
        Ok(self.next_offset - 1)
    }
}

//////////////////////////////////////////////////////////////////////////////
//////////////////////////////////////////////////////////////////////////////

/// An H2 channel wrapper which resolves data frames along with the H2 channel.
pub struct PublisherFut {
    chan: Option<H2DataChannel>,
}

impl PublisherFut {
    /// Create a new instance.
    pub fn new(chan: H2DataChannel) -> Self {
        Self { chan: Some(chan) }
    }
}

impl Future for PublisherFut {
    type Output = Option<Result<(H2DataChannel, Bytes)>>;

    /// Poll the underlying H2 channel for the next data frame.
    ///
    /// This implementation takes care to ensure that the underlying channel is resolved along with
    /// the data frame, which allows for the handler to use the response channel as needed.
    fn poll(mut self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        let mut chan = match self.chan.take() {
            Some(chan) => chan,
            None => return Poll::Ready(None),
        };
        let poll_res = chan.0.poll_data(cx);
        match poll_res {
            Poll::Pending => {
                self.chan = Some(chan);
                Poll::Pending
            }
            Poll::Ready(Some(val)) => Poll::Ready(Some(val.map(move |val| (chan, val)).map_err(From::from))),
            Poll::Ready(None) => {
                self.chan = Some(chan);
                Poll::Ready(None)
            }
        }
    }
}
