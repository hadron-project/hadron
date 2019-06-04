/// A handshake frame holding all data needed for a successful handshake between peers.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Handshake {
    #[prost(uint64, tag="1")]
    pub node_id: u64,
    #[prost(string, tag="2")]
    pub routing_info: std::string::String,
}
