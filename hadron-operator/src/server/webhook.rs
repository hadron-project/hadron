use std::convert::Infallible;
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::http::{header::HeaderName, HeaderMap, HeaderValue, StatusCode};
use axum::routing::{get, post, Router};
use axum::{extract, handler::Handler, AddExtensionLayer};
use hyper::server::conn::Http;
use kube::api::DynamicObject;
use kube::core::admission::{AdmissionResponse, AdmissionReview, Operation};
use metrics_exporter_prometheus::PrometheusHandle;
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;
use tokio_rustls::rustls::{Certificate, NoClientAuth, PrivateKey, ServerConfig};
use tokio_rustls::TlsAcceptor;
use tower_http::trace::TraceLayer;

use crate::config::Config;
use crate::get_metrics_recorder;
use hadron_core::crd::{Pipeline, Stream, Token};

/// The HTTP server.
pub(super) struct HttpServer {
    /// The application's runtime config.
    #[allow(dead_code)]
    config: Arc<Config>,
    /// A channel used for triggering graceful shutdown.
    shutdown_tx: broadcast::Sender<()>,
    /// A channel used for triggering graceful shutdown.
    shutdown_rx: broadcast::Receiver<()>,

    listener: TcpListener,
    acceptor: TlsAcceptor,
}

impl HttpServer {
    /// Construct a new instance.
    pub async fn new(config: Arc<Config>, shutdown: broadcast::Sender<()>) -> Result<Self> {
        let rustls_config = rustls_server_config(config.webhook_key.0.clone(), config.webhook_cert.0.clone()).context("error building webhook TLS config")?;
        let acceptor = TlsAcceptor::from(rustls_config);
        let listener = TcpListener::bind(("0.0.0.0", config.http_port)).await.context("error binding socket address for webhook server")?;

        Ok(Self {
            config,
            shutdown_rx: shutdown.subscribe(),
            shutdown_tx: shutdown,
            listener,
            acceptor,
        })
    }

    pub fn spawn(self) -> JoinHandle<Result<()>> {
        tokio::spawn(self.run())
    }

    async fn run(mut self) -> Result<()> {
        let state = get_metrics_recorder(&self.config).handle();
        let router = Router::new()
            .route("/health", get(|| async { StatusCode::OK }))
            .route("/metrics", get(prom_metrics.layer(AddExtensionLayer::new(state))))
            .route("/k8s/admissions/vaw/pipelines", post(vaw_pipelines.layer(TraceLayer::new_for_http())))
            .route("/k8s/admissions/vaw/streams", post(vaw_streams.layer(TraceLayer::new_for_http())))
            .route("/k8s/admissions/vaw/tokens", post(vaw_tokens.layer(TraceLayer::new_for_http())));

        loop {
            tokio::select! {
                sock_res = self.listener.accept() => {
                    let (stream, _addr) = match sock_res {
                        Ok((stream, addr)) => (stream, addr),
                        Err(err) => {
                            tracing::error!(error = ?err, "error accepting webhook socket connection");
                            let _res = self.shutdown_tx.send(());
                            break;
                        }
                    };
                    let (acceptor, router) = (self.acceptor.clone(), router.clone());
                    tokio::spawn(async move {
                        if let Ok(stream) = acceptor.accept(stream).await {
                            let _res = Http::new().serve_connection(stream, router).await;
                        }
                    });
                },
                _ = self.shutdown_rx.recv() => break,
            }
        }

        Ok(())
    }
}

/// Build RusTLS server config.
fn rustls_server_config(key: PrivateKey, certs: Vec<Certificate>) -> Result<Arc<ServerConfig>> {
    let mut config = ServerConfig::new(NoClientAuth::new());
    config.set_single_cert(certs, key).context("error configuring webhook certificate")?;
    config.set_protocols(&[b"h2".to_vec(), b"http/1.1".to_vec()]);
    Ok(Arc::new(config))
}

/// VAW handler for pipelines.
#[tracing::instrument(level = "debug", skip(payload))]
pub(super) async fn vaw_pipelines(mut payload: extract::Json<AdmissionReview<Pipeline>>) -> std::result::Result<axum::Json<AdmissionReview<DynamicObject>>, Infallible> {
    tracing::debug!(?payload, "received pipeline VAW request");
    let req = match payload.0.request.take() {
        Some(req) => req,
        None => {
            let res = AdmissionResponse::invalid("malformed webhook request received, no `request` field");
            return Ok(axum::Json::from(res.into_review()));
        }
    };

    // Unpack request components based on operation.
    let new_pipeline = match &req.operation {
        // Nothing to do for these, so just accept.
        Operation::Delete | Operation::Connect => {
            return Ok(axum::Json::from(AdmissionResponse::from(&req).into_review()));
        }
        // These operations require at least the new object to be present.
        Operation::Create | Operation::Update => match &req.object {
            Some(new_pipeline) => new_pipeline,
            None => {
                let res = AdmissionResponse::invalid("no pipeline object found in the `object` field, can not validate");
                return Ok(axum::Json::from(res.into_review()));
            }
        },
    };

    // Perform a full validation of the new object.
    if let Err(err) = new_pipeline.validate() {
        let rejection = err.join("; ");
        return Ok(axum::Json::from(AdmissionResponse::invalid(rejection).into_review()));
    }

    // If this is an update request, than validate the compatibility between the new and the old objects.
    if let Some(old_pipeline) = &req.old_object {
        // Should only be populated for Operation::Update.
        if let Err(err) = new_pipeline.validate_compatibility(old_pipeline) {
            let rejection = err.join("; ");
            return Ok(axum::Json::from(AdmissionResponse::invalid(rejection).into_review()));
        }
    }

    Ok(axum::Json::from(AdmissionResponse::from(&req).into_review()))
}

/// VAW handler for streams.
#[tracing::instrument(level = "debug", skip(payload))]
pub(super) async fn vaw_streams(mut payload: extract::Json<AdmissionReview<Stream>>) -> std::result::Result<axum::Json<AdmissionReview<DynamicObject>>, Infallible> {
    tracing::debug!(?payload, "received streams VAW request");
    match payload.0.request.take() {
        Some(req) => Ok(axum::Json::from(AdmissionResponse::from(&req).into_review())),
        None => {
            let res = AdmissionResponse::invalid("malformed webhook request received, no `request` field");
            Ok(axum::Json::from(res.into_review()))
        }
    }
}

/// VAW handler for tokens.
#[tracing::instrument(level = "debug", skip(payload))]
pub(super) async fn vaw_tokens(mut payload: extract::Json<AdmissionReview<Token>>) -> std::result::Result<axum::Json<AdmissionReview<DynamicObject>>, Infallible> {
    tracing::debug!(?payload, "received token VAW request");
    match payload.0.request.take() {
        Some(req) => Ok(axum::Json::from(AdmissionResponse::from(&req).into_review())),
        None => {
            let res = AdmissionResponse::invalid("malformed webhook request received, no `request` field");
            Ok(axum::Json::from(res.into_review()))
        }
    }
}

/// Handler for serving Prometheus metrics.
pub(super) async fn prom_metrics(extract::Extension(state): extract::Extension<PrometheusHandle>) -> (StatusCode, HeaderMap, String) {
    let mut headers = HeaderMap::new();
    headers.insert(HeaderName::from_static("content-type"), HeaderValue::from_static("text/plain; version=0.0.4"));
    (StatusCode::OK, headers, state.render())
}
