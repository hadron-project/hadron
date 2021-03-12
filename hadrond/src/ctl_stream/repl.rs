use anyhow::Result;
use tokio::stream::StreamExt;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use crate::NodeId;

pub struct SpcReplStream {
    id: NodeId,
    target: NodeId,
    input: mpsc::Receiver<()>,
    output: mpsc::Sender<()>,
    repl_shutdown: watch::Receiver<bool>,
}

impl SpcReplStream {
    /// Create a new instance.
    pub fn new(id: NodeId, target: NodeId, input: mpsc::Receiver<()>, output: mpsc::Sender<()>, repl_shutdown: watch::Receiver<bool>) -> Self {
        Self {
            id,
            target,
            input,
            output,
            repl_shutdown,
        }
    }

    pub fn spawn(self) -> JoinHandle<Result<()>> {
        tokio::spawn(self.run())
    }

    async fn run(mut self) -> Result<()> {
        let _: () = futures::future::pending().await;

        /* TODO:
        - send feedback to SPC as batches are replicated.
        - will need to implement a lagging & snapshotting state.
        */

        loop {
            tokio::select! {
                Some(input) = self.input.next() => self.handle_input(input).await,
                Some(needs_shutdown) = self.repl_shutdown.next() => if needs_shutdown { break } else { continue },
            }
        }

        Ok(())
    }

    /// Handle input from the SPC.
    #[tracing::instrument(level = "trace", skip(self))]
    async fn handle_input(&mut self, msg: ()) {
        // TODO: CRITICAL PATH
        //
    }
}
