//! Generate updated proto code for this client.
//!
//! This is setup as an example instead of as part of the build.rs because the protocode does not
//! ship along with the client code when distributed.

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
