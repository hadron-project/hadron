use std::{fmt, error};

use crate::{
    app::AppDataError,
    proto::client::{self, ClientError, ErrorCode},
};

const ERR_HANDSHAKE_REQUIRED: &str = "Client handshake required. Send the ConnectRequest frame.";
const ERR_INTERNAL: &str = "Internal server error.";
const ERR_UNAUTHORIZED: &str = "The given JWT is invalid.";
const ERR_INSUFFICIENT_PERMISSIONS: &str = "The token being used by the client does not have sufficient permissions for the requested operation.";

//////////////////////////////////////////////////////////////////////////////////////////////////
// ClientError ///////////////////////////////////////////////////////////////////////////////////

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl error::Error for ClientError {}

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

    /// Create a new instance representing an `InvalidInput` error.
    pub fn new_invalid_input(message: String) -> Self {
        Self{message, code: ErrorCode::InvalidInput as i32}
    }

    /// Create a new instance representing a `TargetStreamUnknown` error.
    pub fn new_unknown_stream(namespace: String, name: String) -> Self {
        Self{
            message: format!("The target stream {}/{} does not appear to exist. You must create streams before interacting with them.", namespace, name),
            code: ErrorCode::TargetStreamUnknown as i32,
        }
    }
}

impl From<AppDataError> for ClientError {
    fn from(src: AppDataError) -> Self {
        match src {
            AppDataError::Internal => ClientError::new_internal(),
            AppDataError::InvalidInput(msg) => ClientError::new_invalid_input(msg),
            AppDataError::TargetStreamExists => {
                // Why is this bad? This type of error should be translated into a success response for EnsureStream requests.
                log::warn!("Transformed an AppDataError::TargetStreamExists error directly into a client error. This should not take place.");
                ClientError::new_internal()
            }
            AppDataError::UnknownStream{namespace, name} => ClientError::new_unknown_stream(namespace, name),
        }
    }
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// Request Response Extensions ///////////////////////////////////////////////////////////////////

//////////////////////////////////////////////////////////////////////////////
// Connect ///////////////////////////////////////////////////////////////////

impl client::ConnectResponse {
    /// Create a new instance.
    pub fn new(id: String) -> Self {
        Self{response: Some(client::connect_response::Response::Id(id))}
    }

    /// Create a new error instance.
    pub fn err(err: ClientError) -> Self {
        Self{response: Some(client::connect_response::Response::Error(err))}
    }
}

//////////////////////////////////////////////////////////////////////////////
// EnsureStreamResponse //////////////////////////////////////////////////////

impl client::EnsureStreamResponse {
    /// Create a new success instance.
    pub fn new() -> Self {
        Self{error: None}
    }

    /// Create a new error instance.
    pub fn new_err(err: ClientError) -> Self {
        Self{error: Some(err)}
    }
}

//////////////////////////////////////////////////////////////////////////////
// PubStream /////////////////////////////////////////////////////////////////

impl client::PubStreamResponse {
    /// Create a new success instance.
    pub fn new(idx: u64) -> Self {
        Self{result: Some(client::pub_stream_response::Result::Index(idx))}
    }

    /// Create a new error instance.
    pub fn new_err(err: ClientError) -> Self {
        Self{result: Some(client::pub_stream_response::Result::Error(err))}
    }
}
