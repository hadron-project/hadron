//! Schema management.

use anyhow::{Context, Result};
use http::request::Request;
use http::Method;
use proto::v1::{self, SchemaUpdateManaged, SchemaUpdateOneOff, SchemaUpdateRequest, SchemaUpdateResponse};

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
    pub async fn update_schema(&self, schema: &str, branch: &str, timestamp: i64) -> Result<SchemaUpdateResponse> {
        // Build up request.
        let body_req = SchemaUpdateRequest::Managed(SchemaUpdateManaged {
            schema: schema.into(),
            branch: branch.into(),
            timestamp,
        });
        let mut body = self.inner.0.buf.clone().split();
        v1::write_to_bytes(&body_req, &mut body)?;
        let mut builder = Request::builder().method(Method::POST).uri(v1::URL_SCHEMA);
        builder = self.inner.set_request_credentials(builder);
        let req = builder.body(()).context("error building request")?;

        // Open a new H2 channel to send request.
        let mut chan = self.inner.get_channel(None).await?;
        let (rx, mut tx) = chan.send_request(req, false).context("error sending request")?;
        tx.send_data(body.freeze(), true).context("error sending request body")?;
        let res = rx.await.context("error during request")?;
        tracing::info!(res = ?res, "response from server");

        // Check initial headers response & then proceed to stream in body of response.
        let status = res.status();
        let data = res
            .into_body()
            .data()
            .await
            .context("no response body returned")?
            .context("error getting response body")?;
        self.inner.deserialize_response(status, data)
    }

    /// Run a one-off schema update on the cluster.
    ///
    /// TODO: docs
    #[tracing::instrument(level = "debug", skip(self, schema))]
    pub async fn update_schema_oneoff(&self, schema: &str) -> Result<SchemaUpdateResponse> {
        // Build up request.
        let body_req = SchemaUpdateRequest::OneOff(SchemaUpdateOneOff { schema: schema.into() });
        let mut body = self.inner.0.buf.clone().split();
        v1::write_to_bytes(&body_req, &mut body)?;
        let mut builder = Request::builder().method(Method::POST).uri(v1::URL_SCHEMA);
        builder = self.inner.set_request_credentials(builder);
        let req = builder.body(()).context("error building request")?;

        // Open a new H2 channel to send request.
        let mut chan = self.inner.get_channel(None).await?;
        let (rx, mut tx) = chan.send_request(req, false).context("error sending request")?;
        tx.send_data(body.freeze(), true).context("error sending request body")?;
        let res = rx.await.context("error during request")?;
        tracing::info!(res = ?res, "response from server");

        // Check initial headers response & then proceed to stream in body of response.
        let status = res.status();
        let data = res
            .into_body()
            .data()
            .await
            .context("no response body returned")?
            .context("error getting response body")?;
        self.inner.deserialize_response(status, data)
    }
}
