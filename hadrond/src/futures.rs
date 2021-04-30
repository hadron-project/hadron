use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use futures::Stream;
use uuid::Uuid;

use crate::server::H2DataChannel;

/// A stream type used for monitoring the liveness of an H2 data channel.
///
/// When this stream yields an item, this indicates that the channel has closed.
pub struct LivenessStream {
    pub chan: Option<H2DataChannel>,
    pub group: Arc<String>,
    pub chan_id: Uuid,
}

impl Stream for LivenessStream {
    type Item = (Arc<String>, Uuid, H2DataChannel);

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut chan = match self.chan.take() {
            Some(chan) => chan,
            None => return Poll::Ready(None), // This should never be hit.
        };
        match chan.0.poll_data(cx) {
            Poll::Pending => {
                self.chan = Some(chan);
                Poll::Pending
            }
            Poll::Ready(opt) => match opt {
                Some(res) => match res {
                    Ok(data) => {
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
