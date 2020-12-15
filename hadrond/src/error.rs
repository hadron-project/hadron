//! Hadron error abstractions.

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tonic::Status;

/// Applicaiton error variants.
#[derive(Clone, Debug, Error, Serialize, Deserialize)]
pub enum AppError {
    /// The caller is unauthorized to perform the requested action.
    #[error("unauthorized to perform the requested action")]
    Unauthorized,
    /// The caller's token is unknown.
    #[error("the given token is unknown")]
    UnknownToken,
    /// The given input was invalid.
    #[error("validation error: {0}")]
    InvalidInput(String),
}

impl From<AppError> for Status {
    fn from(err: AppError) -> Status {
        (&err).into()
    }
}

impl From<&'_ AppError> for Status {
    fn from(err: &'_ AppError) -> Status {
        match err {
            AppError::Unauthorized => Status::permission_denied(err.to_string()),
            AppError::UnknownToken => Status::unauthenticated(err.to_string()),
            AppError::InvalidInput(_) => Status::invalid_argument(err.to_string()),
        }
    }
}
