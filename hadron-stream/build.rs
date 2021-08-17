use anyhow::{Context, Result};

fn main() -> Result<()> {
    // Build the stream.proto code.
    tonic_build::configure()
        .out_dir("src/models/proto")
        .compile(&["proto/stream.proto"], &["proto"])
        .context("error compiling stream proto")?;

    // Build the pipeline.proto code.
    tonic_build::configure()
        .out_dir("src/models/proto")
        .compile(&["proto/pipeline.proto"], &["proto"])
        .context("error compiling pipeline proto")?;

    // Build the stream.proto code.
    tonic_build::configure()
        .out_dir("src/grpc")
        .build_client(false)
        .build_server(true)
        .compile(&["../proto/stream.proto"], &["../proto"])
        .context("error compiling stream proto")?;

    Ok(())
}
