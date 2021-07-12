//! Futures related library code.

use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use bytes::Bytes;
use futures::Future;

use crate::common::H2Stream;

/// An H2 channel wrapper which resolves data frames along with the H2 channel.
pub struct SubscriberFut {
    node: Arc<String>,
    chan: Option<H2Stream>,
}

impl SubscriberFut {
    /// Create a new instance.
    pub fn new(node: Arc<String>, chan: H2Stream) -> Self {
        Self { node, chan: Some(chan) }
    }
}

impl Future for SubscriberFut {
    type Output = (Arc<String>, Option<(H2Stream, Bytes)>);

    /// Poll the underlying H2 channel for the next data frame.
    ///
    /// This implementation takes care to ensure that the underlying channel is resolved along with
    /// the data frame, which allows for the handler to use the response channel as needed.
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut chan = match self.chan.take() {
            Some(chan) => chan,
            None => return Poll::Ready((self.node.clone(), None)),
        };
        let poll_res = chan.0.poll_data(cx);
        match poll_res {
            Poll::Pending => {
                self.chan = Some(chan);
                Poll::Pending
            }
            Poll::Ready(Some(val)) => match val {
                Ok(data) => Poll::Ready((self.node.clone(), Some((chan, data)))),
                Err(err) => {
                    tracing::error!(node = ?&*self.node, error = ?err, "error while awaiting subscription delivery from node");
                    Poll::Ready((self.node.clone(), None))
                }
            },
            Poll::Ready(None) => {
                self.chan = Some(chan);
                Poll::Ready((self.node.clone(), None))
            }
        }
    }
}
