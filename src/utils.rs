use bytes::BytesMut;
use log::{error};
use prost::Message;

use crate::proto::{peer};

/// Encode the given peer API frame into a bytes buffer.
pub fn encode_peer_frame(frame: &peer::api::Frame) -> BytesMut {
    let mut data = bytes::BytesMut::with_capacity(frame.encoded_len());
    let _ = frame.encode(&mut data).map_err(|err| error!("Failed to serialize protobuf peer API frame. {}", err));
    data
}
