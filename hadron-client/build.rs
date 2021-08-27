use anyhow::{Context, Result};

fn main() -> Result<()> {
    // Build the stream.proto code.
    tonic_build::configure()
        .out_dir("src/grpc")
        .build_client(true)
        .build_server(false)
        .compile(&["../proto/stream.proto"], &["../proto"])
        .context("error compiling stream proto")?;

    // Build the operator.proto code.
    tonic_build::configure()
        .out_dir("src/grpc")
        .build_client(true)
        .build_server(false)
        .compile(&["../proto/operator.proto"], &["../proto"])
        .context("error compiling operator proto")?;

    Ok(())
}
