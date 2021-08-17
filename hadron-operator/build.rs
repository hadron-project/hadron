use anyhow::{Context, Result};

fn main() -> Result<()> {
    // Build the operator.proto code.
    tonic_build::configure()
        .out_dir("src/grpc")
        .build_client(false)
        .build_server(true)
        .compile(&["../proto/operator.proto"], &["../proto"])
        .context("error compiling operator proto")?;

    Ok(())
}
