use anyhow::{Context, Result};

fn main() -> Result<()> {
    // Build the stream.proto code.
    prost_build::Config::new()
        .out_dir("src/models/proto")
        .compile_protos(&["proto/stream.proto"], &["proto"])
        .context("error compiling stream proto")?;

    // Build the pipeline.proto code.
    prost_build::Config::new()
        .out_dir("src/models/proto")
        .compile_protos(&["proto/pipeline.proto"], &["proto"])
        .context("error compiling pipeline proto")?;

    // Build the stream.proto code.
    prost_build::Config::new()
        .out_dir("src/grpc")
        .compile_protos(&["../proto/stream.proto"], &["../proto"])
        .context("error compiling stream proto")?;

    Ok(())
}
