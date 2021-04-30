//! Common library code.

use std::sync::Arc;

use bytes::Bytes;
use h2::client::{handshake, SendRequest};
use h2::{RecvStream, SendStream};

pub(crate) type H2DataChannel = (RecvStream, SendStream<Bytes>);
pub(crate) type H2Channel = SendRequest<Bytes>;
