/// A pipeline stage output record.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct PipelineStageOutput {
    /// The ID of the pipeline instance with which this output is associated.
    #[prost(uint64, required, tag = "1")]
    pub id: u64,
    /// The ID of the parent pipeline object.
    #[prost(string, required, tag = "2")]
    pub pipeline: ::prost::alloc::string::String,
    /// The name of the pipeline stage to which this output corresponds.
    #[prost(string, required, tag = "3")]
    pub stage: ::prost::alloc::string::String,
    /// The data payload of the stage output.
    #[prost(bytes = "vec", required, tag = "4")]
    pub data: ::prost::alloc::vec::Vec<u8>,
}
