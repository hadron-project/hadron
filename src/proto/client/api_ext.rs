use std::{error, fmt};

use actix_raft::AppError;

use super::api::{ClientError, ErrorCode};

const ERR_INTERNAL: &str = "Internal server error.";
const ERR_UNAUTHORIZED: &str = "The given JWT is invalid.";

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
}
