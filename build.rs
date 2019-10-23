use anyhow::{Context, Result};

fn main() -> Result<()> {
    // NOTE WELL: peer protobuf and client protobuf definitions & code are strictly separated
    // to ensure that implementing client drivers stays clean and simple.

    let cwd = std::env::current_dir().context("failed to get current dir")?;
    let proto_dir = cwd.join("protobuf");
    let peer_path = proto_dir.join("peer.proto");
    let client_path = proto_dir.join("client.proto");

    // Compile peer communications protobuf code.
    tonic_build::configure()
        .build_server(true)
        .out_dir("src/proto")
        .compile(&[&peer_path], &[&proto_dir])
        .context("error during peer.proto build")?;

    // Compile client communications protobuf code.
    tonic_build::configure()
        .build_server(true)
        // .type_attribute(".", "#[derive(Serialize, Deserialize)]") // TODO:NOTE: don't think these will be needed, remove soon.
        .out_dir("src/proto")
        .compile(&[&client_path], &[&proto_dir])
        .context("error during client.proto build")?;

    Ok(())
}
