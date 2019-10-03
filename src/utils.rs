use actix::MailboxError;
use bytes::BytesMut;
use log::{error};
use prost::Message;

use crate::{
    app::RgClientPayloadError,
    proto::{client, peer},
};

/// Encode the given peer API frame into a bytes buffer.
pub fn encode_peer_frame(frame: &peer::api::Frame) -> BytesMut {
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
pub fn proto_client_error_from_mailbox_error(err: MailboxError, err_msg: &str) -> client::api::ClientError {
    error!("{} {}", err_msg, err);
    client::api::ClientError::new_internal()
}
