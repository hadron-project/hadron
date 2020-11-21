build-hadrond:
    cargo build --manifest-path hadrond/Cargo.toml --release

kind-cluster:
    kind create cluster --name hadron

build-and-load-kind:
    docker-compose run builder && \
        docker-compose build --no-cache hadron && \
        kind load docker-image --name hadron hadron-local-release:latest

helm-upgrade-kind:
    helm upgrade hadron ./kubernetes/helm -i --set image.fullName=hadron-local-release:latest --set image.pullPolicy=Never

# # Build a release version of Railgun.
# docker-build-release tag='latest':
#     docker build --target release -t railgun:{{tag}} .

# # Build a Railgun container which is optimized for rapid iteration via FS watching. You will want to execute `just run-docker-watcher` after this.
# docker-build-watch:
#     docker build --target builder-watch -t railgun-watch:latest .

# # Run the Railgun watcher container to incrementally compile changes. You will want to execute `just commit-docker-watcher` after this.
# docker-run-watch:
#     docker run --rm -it -v $PWD/src:/railgun/src -v $PWD/Cargo.lock:/railgun/Cargo.lock \
#         -v $PWD/Cargo.toml:/railgun/Cargo.toml -v $PWD/protobuf:/railgun/protobuf -v $PWD/build.rs:/railgun/build.rs \
#         --name railgun-watch railgun-watch

# # Commit the accumulated set of changes in the Railgun watcher container as a new minimal Railgun image in release mode.
# docker-commit-watch:
#     docker commit railgun-watch railgun

# # Kill the Railgun watch container.
# docker-kill-watch:
#     docker kill railgun-watch
