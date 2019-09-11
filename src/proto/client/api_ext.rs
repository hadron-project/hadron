use std::{error, fmt};

use actix_raft::AppError;

use super::api::{ClientError, ErrorCode};

const UNAUTHORIZED_MSG: &str = "The given JWT is invalid.";

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
    /// Create a new instance representing an `Unauthorized` error.
    pub fn new_unauthorized() -> Self {
        Self{
            message: UNAUTHORIZED_MSG.to_string(),
            code: ErrorCode::Unauthorized as i32,
        }
    }
}
