use anyhow::Result;
use axum::http::{header::HeaderName, HeaderMap, HeaderValue, StatusCode};
use axum::{extract::Extension, routing::get, AddExtensionLayer, Router};
use futures::prelude::*;
use metrics_exporter_prometheus::PrometheusHandle;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;

use crate::config::Config;
use crate::get_metrics_recorder;

/// Spawns a prometheus server which uses the default global registry for metrics.
pub fn spawn_prom_server(config: &Config, mut shutdown: broadcast::Receiver<()>) -> JoinHandle<Result<()>> {
    let state = get_metrics_recorder(config).handle();
    let app = Router::new().route("/metrics", get(prometheus_scrape)).layer(AddExtensionLayer::new(state));
    let server = axum::Server::bind(&([0, 0, 0, 0], config.metrics_port).into())
        .serve(app.into_make_service())
        .with_graceful_shutdown(async move {
            let _res = shutdown.recv().await;
        });
    tracing::info!("metrics server is listening at 0.0.0.0:{}/metrics", config.metrics_port);
    tokio::spawn(server.map_err(anyhow::Error::from))
}

/// Handle Prometheus metrics scraping.
async fn prometheus_scrape(Extension(state): Extension<PrometheusHandle>) -> (StatusCode, HeaderMap, String) {
    let mut headers = HeaderMap::new();
    headers.insert(HeaderName::from_static("content-type"), HeaderValue::from_static("text/plain; version=0.0.4"));
    (StatusCode::OK, headers, state.render())
}
