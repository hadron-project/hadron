//! Client V1 protocol code.

pub use crate::client::*;

/// The V1 prefix for all HTTP endpoints; this will appear as `/v1/...` in request paths.
pub const URL_V1: &str = "v1";
/// The V1 URL metadata prefix; this will appear as `/v1/metadata/...` in request paths.
pub const URL_METADATA: &str = "metadata";
/// The V1 URL stream prefix; this will appear as `/v1/stream/...` in request paths.
pub const URL_STREAM: &str = "stream";

/// The V1 URL suffix for publishing to a stream; this will appear as
/// `/v1/stream/{NAMESPACE}/{NAME}/publish` in request paths.
pub const URL_STREAM_PUBLISH: &str = "publish";
/// The V1 URL suffix for subscribing to a stream; this will appear as
/// `/v1/stream/{NAMESPACE}/{NAME}/subscribe` in request paths.
pub const URL_STREAM_SUBSCRIBE: &str = "subscribe";

/// The V1 URL metadata prefix; this will appear as `/v1/metadata/...` in request paths.
pub const ENDPOINT_METADATA_QUERY: &str = "/v1/metadata/query";
/// The V1 endpoint of the update schema handler.
pub const ENDPOINT_METADATA_SCHEMA_UPDATE: &str = "/v1/metadata/schema_update";

pub type SchemaUpdateRequestType = schema_update_request::Type;
pub type StreamPubResponseResult = stream_pub_response::Result;
pub type StreamPubSetupResponseResult = stream_pub_setup_response::Result;
