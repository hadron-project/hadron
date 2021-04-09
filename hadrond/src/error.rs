//! Hadron error abstractions.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Applicaiton error variants.
#[derive(Clone, Debug, Error, Serialize, Deserialize)]
pub enum AppError {
    /// The caller is unauthorized to perform the requested action.
    #[error("unauthorized to perform the requested action")]
    Unauthorized,
    /// The caller's authorization token is unknown.
    #[error("the given authorization token is unknown")]
    UnknownToken,
    /// The caller's credentials are malformed.
    #[error("the given authorization credentials are malformed: {0}")]
    MalformedCredentials(String),
    /// The given input was invalid.
    #[error("validation error: {0}")]
    InvalidInput(String),
    /// The request method is not supported.
    #[error("the request method is not supported")]
    MethodNotSupported,
    /// The resource specified in the path is not found.
    #[error("the resource specified in the path is not found")]
    ResourceNotFound,
}

impl AppError {
    /// Get the HTTP status code and message for this error.
    pub fn status_and_message(&self) -> (http::StatusCode, String) {
        let status = match self {
            AppError::Unauthorized => http::StatusCode::FORBIDDEN,
            AppError::UnknownToken => http::StatusCode::UNAUTHORIZED,
            AppError::MalformedCredentials(_) => http::StatusCode::BAD_REQUEST,
            AppError::InvalidInput(_) => http::StatusCode::BAD_REQUEST,
            AppError::MethodNotSupported => http::StatusCode::METHOD_NOT_ALLOWED,
            AppError::ResourceNotFound => http::StatusCode::NOT_FOUND,
        };
        (status, self.to_string())
    }
}

/// The error type used to indicate that a system shutdown is required.
#[derive(Debug, thiserror::Error)]
#[error("fatal error: {0}")]
pub struct ShutdownError(#[from] pub anyhow::Error);

/// A result type where the error is a `ShutdownError`.
pub type ShutdownResult<T> = ::std::result::Result<T, ShutdownError>;
