#![allow(dead_code)]

use anyhow::{bail, ensure, Result};
use tonic::metadata::AsciiMetadataValue;

use crate::error::AppError;

/// The authorization header basic prefix — for user creds.
const BASIC_PREFIX: &str = "basic ";

pub struct UserCredentials(String);

impl UserCredentials {
    /// Extract a user name & PW hash from the given header.
    pub fn from_auth_header(header: AsciiMetadataValue) -> Result<Self> {
        let header_str = header
            .to_str()
            .map_err(|_| AppError::InvalidCredentials("must be a valid string value".into()))?;

        // Split the header on the basic auth prefix & ensure the leading segment is empty.
        let mut splits = header_str.splitn(2, BASIC_PREFIX);
        ensure!(
            splits.next() == Some(""),
            AppError::InvalidCredentials("authorization header value must begin with 'basic '".into()),
        );

        // Decode the credentials value.
        let datab64 = match splits.next() {
            Some(datab64) if !datab64.is_empty() => datab64,
            _ => bail!(AppError::InvalidCredentials("no basic auth credentials detected in header".into())),
        };
        let creds = match base64::decode(&datab64) {
            Ok(creds_bytes) => match String::from_utf8(creds_bytes) {
                Ok(creds) => creds,
                Err(_) => bail!(AppError::InvalidCredentials(
                    "decoded basic auth credentials were not a valid string value".into()
                )),
            },
            Err(_) => bail!(AppError::InvalidCredentials("could not base64 decode basic auth credentials".into())),
        };
        Ok(UserCredentials(creds))
    }

    /// Extract the username of the credentials, else err if they are malformed.
    pub fn username(&self) -> Result<&str> {
        Ok(self
            .0
            .splitn(2, ':')
            .next()
            .ok_or_else(|| AppError::InvalidCredentials("basic auth credentials were malformed, could not extract username".into()))?)
    }

    /// Extract the password of the credentials, else err if they are malformed.
    pub fn password(&self) -> Result<&str> {
        let mut segs = self.0.splitn(2, ':');
        segs.next();
        Ok(segs
            .next()
            .ok_or_else(|| AppError::InvalidCredentials("basic auth credentials were malformed, could not extract password".into()))?)
    }
}
