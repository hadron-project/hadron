//! Client V1 protocol code.

pub use crate::client::*;

/// The V1 prefix for all HTTP endpoints; this will appear as `/v1/...` in request paths.
pub const URL_V1: &str = "v1";
/// The V1 URL metadata prefix; this will appear as `/v1/metadata/...` in request paths.
pub const URL_METADATA: &str = "metadata";
/// The V1 URL metadata prefix; this will appear as `/v1/metadata/...` in request paths.
pub const ENDPOINT_METADATA_QUERY: &str = "/v1/metadata/query";
/// The V1 endpoint of the update schema handler.
pub const ENDPOINT_METADATA_SCHEMA_UPDATE: &str = "/v1/metadata/schema_update";

pub type SchemaUpdateRequestType = schema_update_request::Type;
