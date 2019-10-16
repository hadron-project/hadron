use actix::MailboxError;
use bincode;
use bytes::BytesMut;
use log::{error};
use prost::Message;

use crate::{
    app::{AppDataError, AppDataResponse, RgClientPayload, RgClientPayloadError},
    proto::peer,
};

//////////////////////////////////////////////////////////////////////////////////////////////////
// Bincode Encoding/Decoding /////////////////////////////////////////////////////////////////////

/// Encode the given payload using bincode.
pub fn bin_encode_client_payload(payload: &RgClientPayload) -> Vec<u8> {
    bincode::serialize(payload)
        .map_err(|err| log::error!("Failed to serialize RgClientPayload using bincode. {:?}", err))
        .unwrap_or(Vec::new())
}

/// Decode the given RgClientPayload using bincode.
pub fn bin_decode_client_payload(buf: Vec<u8>) -> Result<RgClientPayload, AppDataError> {
    bincode::deserialize(buf.as_slice())
        .map_err(|err| {
            log::error!("Failed to deserialize RgClientPayload using bincode. {:?}", err);
            AppDataError::Internal
        })
}

/// Decode an AppDataResponse as a result with an AppDataError.
pub fn bin_decode_app_data_response_as_result(buf: Vec<u8>) -> Result<AppDataResponse, AppDataError> {
    bincode::deserialize(buf.as_slice()).map_err(|err| {
        log::error!("Error deserializing AppDataResponse. {}", err);
        AppDataError::Internal
    })
}

/// Encode an AppDateResponse using bincode.
pub fn bin_encode_app_data_response(data: &AppDataResponse) -> Vec<u8> {
    match bincode::serialize(&data) {
        Ok(val) => val,
        Err(err) => {
            log::error!("Error serializing AppDataResponse. {}", err);
            Vec::new()
        }
    }
}

/// Decode an app data response as a result.
pub fn bin_decode_app_data_error(buf: Vec<u8>) -> AppDataError {
    match bincode::deserialize(buf.as_slice()) {
        Ok(val) => val,
        Err(err) => {
            log::error!("Error deserializing AppDataError. {}", err);
            AppDataError::Internal
        }
    }
}

/// Encode an app data response as a result.
pub fn bin_encode_app_data_error(err: &AppDataError) -> Vec<u8> {
    match bincode::serialize(&err) {
        Ok(val) => val,
        Err(err) => {
            log::error!("Error serializing AppDataError. {}", err);
            Vec::new()
        }
    }
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// Protobuf Encoding/Decoding ////////////////////////////////////////////////////////////////////

/// Encode the given peer API frame into a bytes buffer.
pub fn encode_peer_frame(frame: &peer::Frame) -> BytesMut {
    let mut data = bytes::BytesMut::with_capacity(frame.encoded_len());
    let _ = frame.encode(&mut data).map_err(|err| error!("Failed to serialize protobuf peer API frame. {}", err));
    data
}

/// Transform and log an actix::MailboxError into an ClientError.
pub fn client_error_from_mailbox_error(err: MailboxError, err_msg: &str) -> RgClientPayloadError {
    error!("{} {}", err_msg, err);
    RgClientPayloadError::Internal
}

/// Transform and log an actix::MailboxError into an ClientError.
pub fn app_data_error_from_mailbox_error(err: MailboxError, err_msg: &str) -> AppDataError {
    error!("{} {}", err_msg, err);
    AppDataError::Internal
}
