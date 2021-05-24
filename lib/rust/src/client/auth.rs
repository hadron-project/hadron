//! Schema management.

use anyhow::{bail, Context, Result};
use http::request::Request;
use http::Method;
use prost::Message;
use proto::v1::{self, CreateTokenRequest, CreateTokenResponse, CreateTokenResponseResult, NamespaceGrant};

use crate::Client;

impl Client {
    /// Auth client.
    pub fn auth(&self) -> AuthClient {
        AuthClient { inner: self.clone() }
    }
}

/// Client auth management.
#[derive(Clone)]
pub struct AuthClient {
    inner: Client,
}

impl AuthClient {
    /// Create a new auth token.
    #[tracing::instrument(level = "debug", skip(self))]
    pub async fn create_token(&self, all: bool, metrics: bool, namespaced: Option<Vec<NamespaceGrant>>) -> Result<String> {
        // Build up request.
        let body_req = CreateTokenRequest {
            all,
            metrics,
            namespaced: namespaced.unwrap_or_default(),
        };
        let mut body = self.inner.0.buf.clone().split();
        body_req.encode(&mut body).context("error encoding request")?;
        let mut builder = Request::builder().method(Method::POST).uri(v1::ENDPOINT_METADATA_AUTH_CREATE_TOKEN);
        builder = self.inner.set_request_credentials(builder);
        let req = builder.body(()).context("error building request")?;

        // Open a new H2 channel to send request.
        let mut chan = self.inner.get_channel(None).await?;
        let (rx, mut tx) = chan.send_request(req, false).context("error sending request")?;
        tx.send_data(body.freeze(), true).context("error sending request body")?;
        let res = rx.await.context("error during request")?;
        tracing::info!(status = ?res.status(), headers = ?res.headers(), "response from server");

        // Check initial headers response & then proceed to stream in body of response.
        let status = res.status();
        let data = res
            .into_body()
            .data()
            .await
            .context("no response body returned")?
            .context("error getting response body")?;
        let res: CreateTokenResponse = self.inner.deserialize_response_or_error(status, data)?;
        match res.result {
            Some(CreateTokenResponseResult::Ok(token)) => Ok(token),
            Some(CreateTokenResponseResult::Err(err)) => bail!(err.message),
            None => bail!("malformed response from server, no result variant found in response"),
        }
    }
}
