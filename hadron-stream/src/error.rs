//! Hadron error abstractions.

use tonic::Status;

pub use hadron_core::AppError;

// Error messages.
pub const ERR_ITER_FAILURE: &str = "error returned during key/value iteration from database";
pub const ERR_DB_FLUSH: &str = "error flushing database state";

/// An extension trait for the Hadron core `AppError`.
pub trait AppErrorExt {
    /// Get the HTTP status code and message for this error.
    fn into_status(self) -> Status;

    /// Translate the given error as an app error and map into a gRPC status object.
    fn grpc(err: anyhow::Error) -> Status;
}

impl AppErrorExt for AppError {
    /// Get the HTTP status code and message for this error.
    fn into_status(self) -> Status {
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
    fn grpc(err: anyhow::Error) -> Status {
        err.downcast::<tonic::Status>()
            .or_else(|err| err.downcast::<Self>().map(Self::into_status))
            .unwrap_or_else(|err| Self::Ise(err).into_status())
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
