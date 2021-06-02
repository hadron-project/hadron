use std::pin::Pin;
use std::task::Poll;

use anyhow::{bail, Context, Result};
use futures::{prelude::*, StreamExt};
use tokio::sync::mpsc;
use tonic::Streaming;

use crate::error::{AppError, AppErrorExt, RpcResult, ShutdownError, ERR_DB_FLUSH};
use crate::grpc::{Event, StreamPublishRequest, StreamPublishResponse, WriteAck};
use crate::stream::StreamCtl;
use crate::utils;

/// The CloudEvents spec version currently being used.
const CLOUD_EVENTS_SPEC_VERSION: &str = "1.0";

impl StreamCtl {
    #[tracing::instrument(level = "trace", skip(self, res))]
    pub(super) async fn handle_publisher_request(&mut self, res: Result<(PublisherChan, StreamPublishRequest)>) {
        // Unpack data payload and channel.
        let (chan, data) = match res {
            Ok(chan_and_data) => chan_and_data,
            Err(err) => {
                tracing::error!(error = ?err, "error awaiting data from publisher");
                return; // Channel is dropped here.
            }
        };

        // Publish the new data frame.
        let write_ack = data.ack;
        let last_offset = match self.publish_data_frame(data).await {
            Ok(last_offset) => last_offset,
            Err(err) => {
                tracing::error!(error = ?err, "error while publishing data to stream");
                let status = AppError::grpc(err);
                let _res = chan.0.send(Err(status)).await;
                self.publishers.push(PublisherFut::new(chan));
                return;
            }
        };

        // Respond to the client if no write ack was requested.
        #[allow(clippy::branches_sharing_code)]
        if write_ack == WriteAck::None as i32 {
            let _res = chan.0.send(Ok(StreamPublishResponse { last_offset })).await;
            self.publishers.push(PublisherFut::new(chan));
        }
        // Else, send the channel to a watch group to await async replication acknowledgement.
        else {
            // TODO: impl this. Should not block the task.
            let _res = chan.0.send(Ok(StreamPublishResponse { last_offset })).await;
            self.publishers.push(PublisherFut::new(chan));
        }
    }

    /// Publish a frame of data to the target stream, returning the offset of the last entry written.
    #[tracing::instrument(level = "trace", skip(self, req))]
    async fn publish_data_frame(&mut self, req: StreamPublishRequest) -> Result<u64> {
        tracing::debug!(self.next_offset, "writing data to stream");
        if req.batch.is_empty() {
            bail!(AppError::InvalidInput("entries batch was empty, no-op".into()));
        }

        // Assign an offset to each entry in the payload and write as a batch.
        let mut batch = sled::Batch::default();
        for new_event in req.batch {
            let entry = utils::encode_model(&Event {
                id: self.next_offset,
                source: self.source.clone(),
                specversion: CLOUD_EVENTS_SPEC_VERSION.into(),
                r#type: new_event.r#type,
                subject: new_event.subject,
                optattrs: new_event.optattrs,
                data: new_event.data,
            })
            .context("error encoding stream event record for storage")?;
            batch.insert(&utils::encode_u64(self.next_offset), entry.as_slice());
            self.next_offset += 1;
        }
        self.tree
            .apply_batch(batch)
            .context("error applying write batch")
            .map_err(ShutdownError::from)?;

        // Fsync if requested.
        if req.fsync {
            self.tree
                .flush_async()
                .await
                .context(ERR_DB_FLUSH)
                .map_err(ShutdownError::from)?;
        }

        tracing::debug!(self.next_offset, "finished writing data to stream");
        let offset = self.next_offset - 1;
        let _ = self.offset_signal.send(offset);
        Ok(offset)
    }
}

//////////////////////////////////////////////////////////////////////////////
//////////////////////////////////////////////////////////////////////////////

type PublisherChan = (mpsc::Sender<RpcResult<StreamPublishResponse>>, Streaming<StreamPublishRequest>);

/// An H2 channel wrapper which resolves data frames along with the H2 channel.
pub struct PublisherFut {
    chan: Option<PublisherChan>,
}

impl PublisherFut {
    /// Create a new instance.
    pub fn new(pair: PublisherChan) -> Self {
        Self { chan: Some(pair) }
    }
}

impl Future for PublisherFut {
    type Output = Option<Result<(PublisherChan, StreamPublishRequest)>>;

    /// Poll the underlying H2 channel for the next data frame.
    ///
    /// This implementation takes care to ensure that the underlying channel is resolved along with
    /// the data frame, which allows for the handler to use the response channel as needed.
    fn poll(mut self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> Poll<Self::Output> {
        let mut pair = match self.chan.take() {
            Some(pair) => pair,
            None => return Poll::Ready(None),
        };
        let poll_res = pair.1.poll_next_unpin(cx);
        match poll_res {
            Poll::Pending => {
                self.chan = Some(pair);
                Poll::Pending
            }
            Poll::Ready(Some(val)) => Poll::Ready(Some(val.map(move |val| (pair, val)).map_err(From::from))),
            Poll::Ready(None) => {
                self.chan = Some(pair);
                Poll::Ready(None)
            }
        }
    }
}
