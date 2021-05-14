/// A pipeline instance.
///
/// Pipeline instances are created when the source stream on the same partition receives a new
/// record which matches the pipeline's trigger criteria.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct PipelineInstance {
    /// The ID of this object, which always corresponds to the offset of the source stream from
    /// which this instance was created.
    #[prost(uint64, required, tag="1")]
    pub id: u64,
    /// The ID of the parent pipeline object.
    #[prost(uint64, required, tag="2")]
    pub pipeline_id: u64,
}
/// A pipeline stage output record.
#[derive(Clone, PartialEq, ::prost::Message)]
pub struct PipelineStageOutput {
    /// The ID of the pipeline instance with which this output is associated.
    #[prost(uint64, required, tag="1")]
    pub id: u64,
    /// The ID of the parent pipeline object.
    #[prost(uint64, required, tag="2")]
    pub pipeline_id: u64,
    /// The name of the pipeline stage to which this output corresponds.
    #[prost(string, required, tag="3")]
    pub stage: ::prost::alloc::string::String,
    /// The data payload of the stage output.
    #[prost(bytes="vec", required, tag="4")]
    pub data: ::prost::alloc::vec::Vec<u8>,
}
