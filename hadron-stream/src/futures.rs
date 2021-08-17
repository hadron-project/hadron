use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use futures::{Stream, StreamExt};
use tokio::sync::mpsc;
use tonic::Streaming;
use uuid::Uuid;

// TODO: combine these by making them generic over an additional `data: Debug` type.

/// A stream type used for monitoring the liveness of an H2 data channel.
///
/// When this stream yields an item, this indicates that the channel has closed.
pub struct LivenessStream<Tx, Rx> {
    pub chan: Option<(mpsc::Sender<Tx>, Streaming<Rx>)>,
    pub group: Arc<String>,
    pub chan_id: Uuid,
}

impl<Tx, Rx> Stream for LivenessStream<Tx, Rx> {
    type Item = (Arc<String>, Uuid, (mpsc::Sender<Tx>, Streaming<Rx>));

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut chan = match self.chan.take() {
            Some(chan) => chan,
            None => return Poll::Ready(None), // This should never be hit.
        };
        match chan.1.poll_next_unpin(cx) {
            Poll::Pending => {
                self.chan = Some(chan);
                Poll::Pending
            }
            Poll::Ready(opt) => match opt {
                Some(res) => match res {
                    Ok(_data) => {
                        tracing::warn!(group = ?&*self.group, channel = ?self.chan_id, "protocol error, unexpected data payload received from subscriber");
                        self.chan = Some(chan);
                        Poll::Pending
                    }
                    Err(err) => {
                        tracing::debug!(error = ?err, group = ?&*self.group, channel = ?self.chan_id, "data channel closed");
                        Poll::Ready(Some((self.group.clone(), self.chan_id, chan)))
                    }
                },
                None => Poll::Ready(Some((self.group.clone(), self.chan_id, chan))),
            },
        }
    }
}

/// A stream type used for monitoring the liveness of an H2 data channel for metadata streams.
///
/// When this stream yields an item, this indicates that the channel has closed. This stream type
/// differs from the `LivenessStream` in that it does not have an associated group.
pub struct LivenessStreamMetadata<Tx, Rx> {
    pub chan: Option<(mpsc::Sender<Tx>, Streaming<Rx>)>,
    pub chan_id: Uuid,
}

impl<Tx, Rx> Stream for LivenessStreamMetadata<Tx, Rx> {
    type Item = (Uuid, (mpsc::Sender<Tx>, Streaming<Rx>));

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut chan = match self.chan.take() {
            Some(chan) => chan,
            None => return Poll::Ready(None), // This should never be hit.
        };
        match chan.1.poll_next_unpin(cx) {
            Poll::Pending => {
                self.chan = Some(chan);
                Poll::Pending
            }
            Poll::Ready(opt) => match opt {
                Some(res) => match res {
                    Ok(_data) => {
                        tracing::warn!(channel = ?self.chan_id, "protocol error, unexpected data payload received from subscriber");
                        self.chan = Some(chan);
                        Poll::Pending
                    }
                    Err(err) => {
                        tracing::debug!(error = ?err, channel = ?self.chan_id, "data channel closed");
                        Poll::Ready(Some((self.chan_id, chan)))
                    }
                },
                None => Poll::Ready(Some((self.chan_id, chan))),
            },
        }
    }
}
