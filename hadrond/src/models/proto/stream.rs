/// A stream subscription record.
///
/// NOTE WELL: subscription offsets are stored separately from their model. This helps to optimize
/// updates to offsets as subscribers work through a stream.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct Subscription {
    /// The name of the subscription.
    #[prost(string, required, tag="1")]
    pub group_name: ::prost::alloc::string::String,
    /// The maximum batch size for this subscription.
    #[prost(uint32, required, tag="2")]
    pub max_batch_size: u32,
}
