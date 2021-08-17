//! Hadron error abstractions.

use thiserror::Error;

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
    #[allow(dead_code)]
    #[error("the given authorization username is unknown")]
    UnknownUser,
    /// The caller's credentials are malformed or invalid.
    #[error("the given authorization credentials are malformed or invalid: {0}")]
    InvalidCredentials(String),
    /// The given input was invalid.
    #[error("validation error: {0}")]
    InvalidInput(String),
    /// The resource specified in the path is not found.
    #[error("the resource specified in the path is not found")]
    ResourceNotFound,
    /// The server has hit an internal error, but will remain online.
    #[error("internal server error")]
    Ise(anyhow::Error),
}
