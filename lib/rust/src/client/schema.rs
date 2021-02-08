//! Schema management.

use anyhow::{Context, Result};
use tonic::Request;

use crate::proto::{update_schema_request::Update, UpdateSchemaManaged, UpdateSchemaOneOff, UpdateSchemaRequest, UpdateSchemaResponse};
use crate::Client;

impl Client {
    /// Schema client.
    pub fn schema(&self) -> SchemaClient {
        SchemaClient { inner: self.clone() }
    }
}

/// Client schema management.
#[derive(Clone)]
pub struct SchemaClient {
    inner: Client,
}

impl SchemaClient {
    /// Run a managed schema update on the cluster.
    ///
    /// TODO: docs
    #[tracing::instrument(level = "debug", skip(self, schema, branch, timestamp))]
    pub async fn update_schema(&self, schema: &str, branch: &str, timestamp: i64) -> Result<UpdateSchemaResponse> {
        let proto = UpdateSchemaRequest {
            update: Some(Update::Managed(UpdateSchemaManaged {
                schema: schema.into(),
                branch: branch.into(),
                timestamp,
            })),
        };
        let mut req = Request::new(proto);
        self.inner.set_request_credentials(&mut req);

        Ok(self
            .inner
            .channel()
            .update_schema(req)
            .await
            .context(crate::error::ERR_DURING_REQUEST)?
            .into_inner())
    }

    /// Run a one-off schema update on the cluster.
    ///
    /// TODO: docs
    #[tracing::instrument(level = "debug", skip(self, schema))]
    pub async fn update_schema_oneoff(&self, schema: &str) -> Result<UpdateSchemaResponse> {
        let proto = UpdateSchemaRequest {
            update: Some(Update::Oneoff(UpdateSchemaOneOff { schema: schema.into() })),
        };
        let mut req = Request::new(proto);
        self.inner.set_request_credentials(&mut req);

        Ok(self
            .inner
            .channel()
            .update_schema(req)
            .await
            .context(crate::error::ERR_DURING_REQUEST)?
            .into_inner())
    }
}
