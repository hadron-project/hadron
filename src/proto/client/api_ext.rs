use std::{error, fmt};

use actix_raft::AppError;

use crate::{
    proto::client::api::{self, ClientError, ErrorCode},
};

const ERR_HANDSHAKE_REQUIRED: &str = "Client handshake required. Send the ConnectRequest frame.";
const ERR_INTERNAL: &str = "Internal server error.";
const ERR_UNAUTHORIZED: &str = "The given JWT is invalid.";
const ERR_INSUFFICIENT_PERMISSIONS: &str = "The token being used by the client does not have sufficient permissions for the requested operation.";

//////////////////////////////////////////////////////////////////////////////////////////////////
// ClienntError //////////////////////////////////////////////////////////////////////////////////

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl error::Error for ClientError {}

impl AppError for ClientError {}

impl ClientError {
    /// Create a new instance representing a `HandshakeRequired` error.
    pub fn new_handshake_required() -> Self {
        Self{
            message: ERR_HANDSHAKE_REQUIRED.to_string(),
            code: ErrorCode::HandshakeRequired as i32,
        }
    }
    /// Create a new instance representing an `Internal` error.
    pub fn new_internal() -> Self {
        Self{
            message: ERR_INTERNAL.to_string(),
            code: ErrorCode::Internal as i32,
        }
    }

    /// Create a new instance representing an `Unauthorized` error.
    pub fn new_unauthorized() -> Self {
        Self{
            message: ERR_UNAUTHORIZED.to_string(),
            code: ErrorCode::Unauthorized as i32,
        }
    }

    /// Create a new instance representing an `InsufficientPermissions` error.
    pub fn new_insufficient_permissions() -> Self {
        Self{
            message: ERR_INSUFFICIENT_PERMISSIONS.to_string(),
            code: ErrorCode::InsufficientPermissions as i32,
        }
    }

    /// Create a new insntance representing a `TargetStreamUnknown` error.
    pub fn new_unknown_stream(namespace: &str, name: &str) -> Self {
        Self{
            message: format!("The target stream {} in namespace {} does not appear to exist. You must create streams before interacting with them.", name, namespace),
            code: ErrorCode::TargetStreamUnknown as i32,
        }
    }
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// Request Response Extensions ///////////////////////////////////////////////////////////////////

//////////////////////////////////////////////////////////////////////////////
// PubStreamResponse /////////////////////////////////////////////////////////

impl api::PubStreamResponse {
    /// Create a new success instance.
    pub fn new(idx: u64) -> Self {
        Self{result: Some(api::pub_stream_response::Result::Index(idx))}
    }

    /// Create a new error instance.
    pub fn new_err(err: ClientError) -> Self {
        Self{result: Some(api::pub_stream_response::Result::Error(err))}
    }
}
