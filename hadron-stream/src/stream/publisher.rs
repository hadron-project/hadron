use anyhow::{bail, Context, Result};
use tokio::sync::oneshot;

use crate::error::{AppError, AppErrorExt, RpcResult, ShutdownError, ERR_DB_FLUSH};
use crate::grpc::{Event, StreamPublishRequest, StreamPublishResponse};
use crate::stream::StreamCtl;
use crate::utils;

impl StreamCtl {
    #[tracing::instrument(level = "trace", skip(self, tx, data))]
    pub(super) async fn handle_publisher_request(&mut self, tx: oneshot::Sender<RpcResult<StreamPublishResponse>>, data: StreamPublishRequest) {
        // Publish the new data frame.
        let _write_ack = data.ack;
        let offset = match self.publish_data_frame(data).await {
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
                specversion: utils::CLOUD_EVENTS_SPEC_VERSION.into(),
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
        let _ = self.offset_signal.send(self.next_offset);
        Ok(self.next_offset - 1)
    }
}
