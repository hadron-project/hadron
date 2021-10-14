use anyhow::{bail, Context, Result};
use tokio::sync::{oneshot, watch};

use crate::error::{AppError, AppErrorExt, RpcResult, ShutdownError, ERR_DB_FLUSH};
use crate::grpc::{StreamPublishRequest, StreamPublishResponse};
use crate::stream::StreamCtl;
use crate::utils;

impl StreamCtl {
    #[tracing::instrument(level = "trace", skip(self, tx, data))]
    pub(super) async fn handle_publisher_request(&mut self, tx: oneshot::Sender<RpcResult<StreamPublishResponse>>, data: StreamPublishRequest) {
        // Publish the new data frame.
        let _write_ack = data.ack;
        let offset = match Self::publish_data_frame(&self.tree, &mut self.current_offset, &self.offset_signal, data).await {
            Ok(offset) => offset,
            Err(err) => {
                tracing::error!(error = ?err, "error while publishing data to stream");
                let status = AppError::grpc(err);
                let _res = tx.send(Err(status));
                return;
            }
        };

        // Respond to the client if no write ack was requested.
        let _res = tx.send(Ok(StreamPublishResponse { offset }));
        // if write_ack == WriteAck::None as i32 {
        //     let _res = tx.send(Ok(StreamPublishResponse { last_offset }));
        // }
        // else {
        //     // Else, send the channel to a watch group to await async replication acknowledgement.
        //     // TODO: impl replication, and do not block this task.
        // }
    }

    /// Publish a frame of data to the target stream, returning the offset of the last entry written.
    #[tracing::instrument(level = "trace", skip(tree, current_offset, offset_signal, req))]
    pub(super) async fn publish_data_frame(
        tree: &sled::Tree, current_offset: &mut u64, offset_signal: &watch::Sender<u64>, req: StreamPublishRequest,
    ) -> Result<u64> {
        tracing::debug!("writing data to stream");
        if req.batch.is_empty() {
            bail!(AppError::InvalidInput("entries batch was empty, no-op".into()));
        }

        // Assign an offset to each entry in the payload and write as a batch.
        let mut batch = sled::Batch::default();
        for new_event in req.batch {
            *current_offset += 1;
            let entry = utils::encode_model(&new_event).context("error encoding stream event record for storage")?;
            batch.insert(&utils::encode_u64(*current_offset), entry.as_slice());
        }
        tree.apply_batch(batch)
            .context("error applying write batch")
            .map_err(ShutdownError::from)?;

        // Fsync if requested.
        if req.fsync {
            tree.flush_async()
                .await
                .context(ERR_DB_FLUSH)
                .map_err(ShutdownError::from)?;
        }

        tracing::debug!(current_offset, "finished writing data to stream");
        let _ = offset_signal.send(*current_offset);
        Ok(*current_offset)
    }
}
