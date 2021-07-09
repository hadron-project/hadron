//! Events created by the K8s controller and observed by other components of the system.

use std::sync::Arc;

use crate::crd::{Pipeline, Stream, Token};

/// An event describing some state change in the system.
#[derive(Clone)]
pub enum CrdStateChange {
    StreamUpdated(Arc<String>, Arc<Stream>),
    StreamDeleted(Arc<String>, Arc<Stream>),
    PipelineUpdated(Arc<String>, Arc<Pipeline>, Arc<Stream>),
    PipelineDeleted(Arc<String>, Arc<Pipeline>),
    TokenUpdated(Arc<String>, Arc<Token>),
    TokenDeleted(Arc<String>, Arc<Token>),
}
