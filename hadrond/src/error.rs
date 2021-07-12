//! Hadron error abstractions.

use thiserror::Error;
use tonic::Status;

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

impl AppError {
    /// Get the HTTP status code and message for this error.
    pub fn into_status(&self) -> Status {
        match self {
            AppError::Unauthorized => Status::unauthenticated(self.to_string()),
            AppError::UnknownToken | AppError::UnknownUser => Status::permission_denied(self.to_string()),
            AppError::InvalidCredentials(_) => Status::permission_denied(self.to_string()),
            AppError::InvalidInput(_) => Status::invalid_argument(self.to_string()),
            AppError::ResourceNotFound => Status::not_found(self.to_string()),
            AppError::Ise(_) => Status::internal(self.to_string()),
        }
    }

    /// Translate the given error as an app error and map into a gRPC status object.
    pub fn grpc(err: anyhow::Error) -> Status {
        err.downcast::<Self>().unwrap_or_else(|err| Self::Ise(err)).into_status()
    }
}

/// The error type used to indicate that a system shutdown is required.
#[derive(Debug, thiserror::Error)]
#[error("fatal error: {0}")]
pub struct ShutdownError(#[from] pub anyhow::Error);

/// A result type where the error is a `ShutdownError`.
pub type ShutdownResult<T> = ::std::result::Result<T, ShutdownError>;

/// A result type used with the gRPC system.
pub type RpcResult<T> = ::std::result::Result<T, tonic::Status>;
