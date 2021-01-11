use anyhow::{Context, Result};

fn main() -> Result<()> {
    // NOTE WELL: peer protobuf and client protobuf definitions & code are strictly separated
    // to ensure that implementing client drivers stays clean and simple.

    let cwd = std::env::current_dir().context("failed to get current dir")?;
    let proto_dir = cwd.join("..").join("protobuf");
    let peer_path = proto_dir.join("peer.proto");
    let client_path = proto_dir.join("client.proto");
    let storage_path = proto_dir.join("storage.proto");

    // Compile peer communications protobuf code.
    tonic_build::configure()
        .build_server(true)
        .out_dir("src/proto")
        .compile(&[&peer_path], &[&proto_dir])
        .context("error during peer.proto build")?;

    // Compile client communications protobuf code.
    tonic_build::configure()
        .build_server(true)
        .type_attribute(".", "#[derive(serde::Serialize, serde::Deserialize)]")
        .out_dir("src/proto")
        .compile(&[&client_path], &[&proto_dir])
        .context("error during client.proto build")?;

    // Compile storage protobuf code.
    tonic_build::configure()
        .build_server(false)
        .build_client(false)
        .out_dir("src/proto")
        .compile(&[&storage_path], &[&proto_dir])
        .context("error during storage.proto build")?;

    Ok(())
}
