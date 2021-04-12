use anyhow::{Context, Result};

fn main() -> Result<()> {
    prost_build::Config::new()
        .out_dir("src")
        .compile_protos(&["client.proto"], &["."])
        .context("error compiling protos")?;
    Ok(())
}
