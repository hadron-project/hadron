//! Hadron error abstractions.

use thiserror::Error;

// Error messages.
pub const ERR_ITER_FAILURE: &str = "error returned during key/value iteration from database";
pub const ERR_DB_FLUSH: &str = "error flushing database state";

/// Applicaiton error variants.
#[derive(Debug, Error)]
pub enum AppError {
    /// The caller is unauthorized to perform the requested action.
    #[error("unauthorized to perform the requested action")]
    Unauthorized,
    /// The caller's authorization token is unknown.
    #[error("the given authorization token is unknown")]
    UnknownToken,
    /// The caller's authorization username is unknown.
    #[error("the given authorization username is unknown")]
    UnknownUser,
    /// The caller's credentials are malformed or invalid.
    #[error("the given authorization credentials are malformed or invalid: {0}")]
    InvalidCredentials(String),
    /// The given input was invalid.
    #[error("validation error: {0}")]
    InvalidInput(String),
    /// The request method is not allowed.
    #[error("the request method is not allowed")]
    MethodNotAllowed,
    /// The resource specified in the path is not found.
    #[error("the resource specified in the path is not found")]
    ResourceNotFound,
    /// The server has hit an internal error, but will remain online.
    #[error("internal server error")]
    Ise(#[from] anyhow::Error),
}

impl AppError {
    /// Get the HTTP status code and message for this error.
    pub fn status_and_message(&self) -> (http::StatusCode, String) {
        let status = match self {
            AppError::Unauthorized => http::StatusCode::FORBIDDEN,
            AppError::UnknownToken | AppError::UnknownUser => http::StatusCode::UNAUTHORIZED,
            AppError::InvalidCredentials(_) => http::StatusCode::UNAUTHORIZED,
            AppError::InvalidInput(_) => http::StatusCode::BAD_REQUEST,
            AppError::MethodNotAllowed => http::StatusCode::METHOD_NOT_ALLOWED,
            AppError::ResourceNotFound => http::StatusCode::NOT_FOUND,
            AppError::Ise(_) => http::StatusCode::INTERNAL_SERVER_ERROR,
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
