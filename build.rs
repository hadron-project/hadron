use prost_build;

fn main() {
    // Compile storage protobuf code.
    let _ = prost_build::Config::new()
        .out_dir("src/proto/storage")
        .compile_protos(&[
            "protobuf/storage/raft.proto",
        ], &[
            "protobuf/storage",
        ])
        .map_err(|err| panic!("Failed to compile storage protobuf code. {}", err));

    // Compile peer communications protobuf code.
    let _ = prost_build::Config::new()
        .out_dir("src/proto/peer")
        .compile_protos(&[
            "protobuf/peer/api.proto",
            "protobuf/peer/handshake.proto",
        ], &[
            "protobuf/peer",
        ])
        .map_err(|err| panic!("Failed to compile peer protobuf code. {}", err));

    // Compile client communications protobuf code.
    let _ = prost_build::Config::new()
        .out_dir("src/proto/client")
        .type_attribute("api.ClientError", "#[derive(Serialize, Deserialize)]")
        .compile_protos(&[
            "protobuf/client/api.proto",
        ], &[
            "protobuf/client",
        ])
        .map_err(|err| panic!("Failed to compile client protobuf code. {}", err));
}
