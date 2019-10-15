use prost_build;

fn main() {
    // NOTE WELL: peer protobuf and client protobuf definitions & code are strictly separated
    // to ensure that implementing client drivers stays clean and simple.

    // Compile peer communications protobuf code.
    let _ = prost_build::Config::new()
        .out_dir("src/proto")
        .compile_protos(&[
            "protobuf/peer.proto",
        ], &[
            "protobuf",
        ])
        .map_err(|err| panic!("Failed to compile peer protobuf code. {}", err));

    // Compile client communications protobuf code.
    let _ = prost_build::Config::new()
        .out_dir("src/proto")
        .type_attribute(".", "#[derive(Serialize, Deserialize)]")
        .compile_protos(&[
            "protobuf/client.proto",
        ], &[
            "protobuf",
        ])
        .map_err(|err| panic!("Failed to compile client protobuf code. {}", err));
}
