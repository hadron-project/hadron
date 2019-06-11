use std::{error, fmt};

use actix_raft::AppError;

use super::api::{ClientError};

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl error::Error for ClientError {}

impl AppError for ClientError {}
