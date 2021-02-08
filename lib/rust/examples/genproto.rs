//! Generate the protobuf code used by the client.

use anyhow::{Context, Result};

fn main() -> Result<()> {
    // Compile client communications protobuf code.
    tonic_build::configure()
        .build_server(true)
        .out_dir("src/proto")
        .compile(&["../../protobuf/client.proto"], &["../../protobuf"])
        .context("error during client.proto build")?;

    Ok(())
}
