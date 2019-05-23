/// A handshake frame holding all data needed for a successful handshake between peers.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Handshake {
    #[prost(string, tag="1")]
    pub node_id: std::string::String,
    #[prost(string, tag="2")]
    pub routing_info: std::string::String,
}
