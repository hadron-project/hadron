//! A module encapsulating the `WsClient` actor and its logic.
//!
//! The `WsClient` actor represents a WebSocket connection which was initialized by a client. The
//! client may be written in any number of different languages, but all clients must adhere to the
//! Railgun Client Wire Protocol in order to successfully communicate with the cluster.
//!
//! Client requests may be forwarded along to other members of the cluster as needed in order
//! to satisfy the clients request.

use std::{
    time::{Duration, Instant},
};

use actix::*;
use actix_web_actors::ws;
use log::{debug, info, warn};
use uuid;

use crate::{
    NodeId,
    networking::{Network},
};

enum ClientState {
    Initial,
    Active(ClientStateActive),
}

struct ClientStateActive {

}

//////////////////////////////////////////////////////////////////////////////////////////////////
// WsClient //////////////////////////////////////////////////////////////////////////////////////

/// An actor responsible for handling inbound WebSocket connections from clients.
pub(super) struct WsClient {
    /// The ID of this node.
    node_id: NodeId,

    /// Address of the parent `Network` actor.
    parent: Addr<Network>,

    /// The ID assigned to this connection.
    connection_id: String,

    /// The last successful heartbeat on this socket. Will reckon the client as being dead after
    /// `CLIENT_HB_THRESHOLD` has been exceeded since last successful heartbeat.
    heartbeat: Instant,

    /// A handle to the heartbeat interval job.
    heartbeat_handle: Option<SpawnHandle>,

    /// The state of the client connection.
    state: ClientState,

    /// The configured liveness threshold for this client connection.
    ///
    /// This value may be updated by clients during the handshake.
    client_death_threshold: Duration,

    // /// A map of pending stream deliveries awaiting a client response.
    // stream_outbound: HashMap<String, (oneshot::Sender<Result<api::Response, ()>>, SpawnHandle)>, // TODO: update type.
}

impl WsClient {
    /// Create a new instance.
    pub fn new(parent: Addr<Network>, node_id: NodeId, client_death_threshold: Duration) -> Self {
        Self{
            node_id, parent,
            connection_id: uuid::Uuid::new_v4().to_string(),
            heartbeat: Instant::now(),
            heartbeat_handle: None,
            state: ClientState::Initial,
            client_death_threshold,
        }
    }

    // /// Sever the connection with the peer after sending a disconnect frame.
    // fn disconnect(&mut self, disconnect: api::Disconnect, ctx: &mut ws::WebsocketContext<Self>) {
    //     debug!("Peer connection needs disconnect: {}. Closing.", disconnect as i32);
    //     use prost::Message;
    //     let frame = api::Frame{meta: None, payload: Some(api::frame::Payload::Disconnect(disconnect as i32))};
    //     let mut data = bytes::BytesMut::with_capacity(frame.encoded_len());
    //     let _ = frame.encode(&mut data).map_err(|err| error!("Failed to serialize protobuf frame. {}", err));
    //     ctx.binary(data);
    //     ctx.stop();
    // }

    // /// Handle peer handshake protocol.
    // ///
    // /// A handshake frame has been received. Update the peer ID based on the given information,
    // /// propagate all of this information to the parent `Network` actor, and then response to
    // /// the caller with a handshake frame as well.
    // fn handshake(&mut self, hs: api::Handshake, meta: api::Meta, ctx: &mut ws::WebsocketContext<Self>) {
    //     // If the connection is being made with self due to initial discovery probe, then respond
    //     // over the socket with a disconnect frame indicating that such is the case.
    //     if hs.node_id == self.node_id {
    //         use prost::Message;
    //         let frame = api::Frame::new_disconnect(api::Disconnect::ConnectionInvalid, meta);
    //         let mut buf = bytes::BytesMut::with_capacity(frame.encoded_len());
    //         let _ = frame.encode(&mut buf).map_err(|err| error!("Failed to encode protobuf frame. {}", err));
    //         ctx.binary(buf);
    //         return ctx.stop();
    //     }

    //     // Update handshake state & peer ID.
    //     self.state = PeerHandshakeState::Done;
    //     self.peer_id = Some(hs.node_id);

    //     // Propagate handshake info to parent `Network` actor. If disconnect is needed, send
    //     // disconnect frame over to peer so that it will not attempt to reconnect.
    //     let f = self.parent.send(PeerConnectionLive{peer_id: hs.node_id, routing_info: hs.routing_info, addr: PeerAddr::FromPeer(ctx.address())});
    //     let af = actix::fut::wrap_future(f)
    //         .map_err(|_, _, _| ()) // NOTE: would only get hit on a timeout or closed. Neither will be hit.
    //         .and_then(|res, iself: &mut Self, ictx: &mut ws::WebsocketContext<Self>| match res {
    //             Ok(()) => actix::fut::ok(()),
    //             Err(disconnect) => {
    //                 iself.disconnect(disconnect, ictx);
    //                 actix::fut::err(())
    //             }
    //         })
    //         .map(move |_, iself, ictx| iself.handshake_response(meta, ictx));
    //     ctx.spawn(af);
    // }

    // /// Handle sending a handshake response.
    // fn handshake_response(&mut self, meta: api::Meta, ctx: &mut ws::WebsocketContext<Self>) {
    //     // Respond to the caller with a handshake frame.
    //     // TODO: finish up the routing info pattern. See the peer connection management doc.
    //     use prost::Message;
    //     let hs_out = api::Handshake{node_id: self.node_id, routing_info: String::with_capacity(0)};
    //     let frame = api::Frame{meta: Some(meta), payload: Some(api::frame::Payload::Response(api::Response{
    //         segment: Some(api::response::Segment::Handshake(hs_out)),
    //     }))};
    //     let mut data = bytes::BytesMut::with_capacity(frame.encoded_len());
    //     let _ = frame.encode(&mut data).map_err(|err| error!("Failed to serialize protobuf frame. {}", err));
    //     ctx.binary(data);
    // }

    /// Perform a healthcheck at the given interval.
    fn healthcheck(&mut self, ctx: &mut ws::WebsocketContext<Self>) {
        if let Some(handle) = self.heartbeat_handle.take() {
            ctx.cancel_future(handle);
        }
        self.heartbeat_handle = Some(ctx.run_interval(self.client_death_threshold, |act, ctx| {
            if Instant::now().duration_since(act.heartbeat) > act.client_death_threshold {
                info!("Client connection {} appears to be dead, disconnecting.", act.connection_id);
                ctx.stop();
            }
        }));
    }
}

impl Actor for WsClient {
    type Context = ws::WebsocketContext<Self>;

    /// Logic for starting this actor.
    ///
    /// Clients are responsible for driving the heartbeat / healtcheck system. The server will
    /// simply check for liveness at the configured interval.
    fn started(&mut self, ctx: &mut Self::Context) {
        self.healthcheck(ctx);
    }
}

impl StreamHandler<ws::Message, ws::ProtocolError> for WsClient {
    /// Handle messages received over the WebSocket.
    fn handle(&mut self, msg: ws::Message, ctx: &mut Self::Context) {
        match msg {
            ws::Message::Nop => (),
            ws::Message::Close(reason) => debug!("Connection with client {} is closing. {:?}", self.connection_id, reason),
            ws::Message::Ping(_) => {
                // Heartbeat response received from connected client.
                self.heartbeat = Instant::now();
            }
            ws::Message::Pong(_) => warn!("Protocol error. Unexpectedly received a pong frame from connected client."),
            ws::Message::Text(_) => warn!("Protocol error. Unexpectedly received a text frame from connected client."),
            ws::Message::Binary(_data) => {
                debug!("Handling binary frame from connected client.");
            }
        }
    }
}
