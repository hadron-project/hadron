# Invoke `cargo build` with any supplied arguments.
build +args='':
    cargo build {{args}}


# Execute `cargo clippy` with any supplied arguments
clip +args='':
    cargo clippy

# Execute `cargo fmt` with any supplied arguments
fmt +args='':
    cargo fmt {{args}}

# Execute `cargo fmt` emitting only to stdout.
fmt-console:
    cargo fmt -- --emit stdout

# Build a release version of Railgun.
docker-build-release tag='latest':
    docker build --target release -t railgun:{{tag}} .

# Build a Railgun container which is optimized for rapid iteration via FS watching. You will want to execute `just run-docker-watcher` after this.
docker-build-watch:
    docker build --target builder-watch -t railgun-watch:latest .

# Run the Railgun watcher container to incrementally compile changes. You will want to execute `just commit-docker-watcher` after this.
docker-run-watch:
    docker run --rm -it -v $PWD/src:/railgun/src -v $PWD/Cargo.lock:/railgun/Cargo.lock \
        -v $PWD/Cargo.toml:/railgun/Cargo.toml -v $PWD/protobuf:/railgun/protobuf -v $PWD/build.rs:/railgun/build.rs \
        --name railgun-watch railgun-watch

# Commit the accumulated set of changes in the Railgun watcher container as a new minimal Railgun image in release mode.
docker-commit-watch:
    docker commit railgun-watch railgun

# Kill the Railgun watch container.
docker-kill-watch:
    docker kill railgun-watch