use prost_build;

fn main() {
    // Compile internal API protobuf code.
    let _ = prost_build::Config::new()
        .out_dir("src/proto/peer")
        .compile_protos(&[
            "protobuf/peer/api.proto",
            "protobuf/peer/handshake.proto",
        ], &[
            "protobuf/peer",
        ])
        .map_err(|err| panic!("Failed to compile internal protobuf code. {}", err));

    // Compile public API protobuf code.
    let _ = prost_build::Config::new()
        .out_dir("src/proto/client")
        .compile_protos(&[
            "protobuf/client/api.proto",
        ], &[
            "protobuf/client",
        ])
        .map_err(|err| panic!("Failed to compile protobuf code. {}", err));
}
