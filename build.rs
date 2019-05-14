use prost_build;

fn main() {
    // Compile internal API protobuf code.
    let _ = prost_build::Config::new()
        .out_dir("src/api/internal")
        .compile_protos(&[
            "protobuf/internal/api.proto",
            "protobuf/internal/handshake.proto",
        ], &[
            "protobuf/internal",
        ])
        .map_err(|err| panic!("Failed to compile internal protobuf code. {}", err));

    // Compile public API protobuf code.
    let _ = prost_build::Config::new()
        .out_dir("src/api/public")
        .compile_protos(&[
            "protobuf/public/api.proto",
        ], &[
            "protobuf/public",
        ])
        .map_err(|err| panic!("Failed to compile protobuf code. {}", err));
}
