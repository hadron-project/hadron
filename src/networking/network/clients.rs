use actix::prelude::*;

use crate::{
    auth::{Claims, ClaimsV1},
    networking::{Network, client::VerifyToken},
    proto::{client::ClientError, peer},
};

//////////////////////////////////////////////////////////////////////////////////////////////////
// VerifyToken ///////////////////////////////////////////////////////////////////////////////////

impl Handler<VerifyToken> for Network {
    type Result = ResponseActFuture<Self, Claims, ClientError>;

    fn handle(&mut self, msg: VerifyToken, _ctx: &mut Context<Self>) -> Self::Result {
        // TODO: implement this. See #24.
        if msg.0.len() != 0 {
            Box::new(fut::err(ClientError::new_unauthorized()))
        } else {
            Box::new(fut::ok(Claims::V1(ClaimsV1::Root)))
        }
    }
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// GetRoutingInfo ////////////////////////////////////////////////////////////////////////////////

/// A request to get the `Network` actor's latest routing info.
pub(in crate::networking) struct GetRoutingInfo;

impl Message for GetRoutingInfo {
    type Result = Result<peer::RoutingInfo, ()>;
}

impl Handler<GetRoutingInfo> for Network {
    type Result = Result<peer::RoutingInfo, ()>;

    fn handle(&mut self, _: GetRoutingInfo, _: &mut Self::Context) -> Self::Result {
        Ok(self.routing.clone())
    }
}

//////////////////////////////////////////////////////////////////////////////////////////////////
// ClientConnectionLive //////////////////////////////////////////////////////////////////////////

/// A message indicating that a new client connection has been established, or that an old one has been disconnected.
pub(in crate::networking) enum ClientConnectionUpdate {
    /// An update indicating that a new client connection has been established to this node.
    #[allow(dead_code)]
    Connected(String),
    /// An update indicating that the specified client connection has been disconnected.
    #[allow(dead_code)]
    Disconnected(String),
}

impl Message for ClientConnectionUpdate {
    type Result = ();
}

impl Handler<ClientConnectionUpdate> for Network {
    type Result = ();

    fn handle(&mut self, msg: ClientConnectionUpdate, _: &mut Self::Context) {
        // TODO:
        // - need to update router.
        // - need to flood this node's updated routing info to all peers.
        match msg {
            ClientConnectionUpdate::Connected(_) => (),
            ClientConnectionUpdate::Disconnected(_) => (),
        }
    }
}
