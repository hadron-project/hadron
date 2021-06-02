//! Client V1 protocol code.

pub use crate::client::*;

/// The V1 prefix for all HTTP endpoints; this will appear as `/v1/...` in request paths.
pub const URL_V1: &str = "v1";
/// The V1 URL metadata prefix; this will appear as `/v1/metadata/...` in request paths.
pub const URL_METADATA: &str = "metadata";
/// The V1 URL stream prefix; this will appear as `/v1/stream/...` in request paths.
pub const URL_STREAM: &str = "stream";
/// The V1 URL pipeline prefix; this will appear as `/v1/pipeline/...` in request paths.
pub const URL_PIPELINE: &str = "pipeline";

/// The V1 URL suffix for publishing to a stream; this will appear as
/// `/v1/stream/{NAME}/{PARTITION}/publish` in request paths.
pub const URL_STREAM_PUBLISH: &str = "publish";
/// The V1 URL suffix for subscribing to a stream; this will appear as
/// `/v1/stream/{NAME}/{PARTITION}/subscribe` in request paths.
pub const URL_STREAM_SUBSCRIBE: &str = "subscribe";

/// The full V1 endpoint for establishing a metadata subscription.
pub const ENDPOINT_METADATA_SUBSCRIBE: &str = "/v1/metadata/subscribe";

pub type StreamPubResponseResult = stream_pub_response::Result;
pub type StreamPubSetupResponseResult = stream_pub_setup_response::Result;

pub type StreamSubSetupRequestStartingPoint = stream_sub_setup_request::StartingPoint;
pub type StreamSubSetupResponseResult = stream_sub_setup_response::Result;
pub type StreamSubDeliveryResponseResult = stream_sub_delivery_response::Result;

pub type PipelineSubSetupResponseResult = pipeline_sub_setup_response::Result;
pub type PipelineSubDeliveryResponseResult = pipeline_sub_delivery_response::Result;

pub type MetadataChangeType = metadata_change::Change;
pub type MetadataSubSetupResponseResult = metadata_sub_setup_response::Result;
