ARG TAG=1.37.0-slim-stretch
FROM rust:${TAG} as base

LABEL maintainer="Anthony Dodd"
WORKDIR /railgun

##############################################################################
# builder-watch ##############################################################
FROM base as builder-watch
RUN cargo install cargo-watch --version 7.2.1

# NOTE WELL: in order for this to be useful, the continer running this stage's command should
# have src, Cargo.toml, Cargo.lock, build.rs & protobuf volume mounted into the container.

CMD ["cargo", "watch", "-i", "src/proto/*", "-s", "cargo build --release && mv /railgun/target/release/railgun /bin/railgun"]

##############################################################################
# builder-release ############################################################
FROM base as builder-release

COPY ./src src
COPY ./Cargo.lock Cargo.lock
COPY ./Cargo.toml Cargo.toml
COPY ./build.rs build.rs
COPY ./protobuf protobuf
RUN cargo build --release

##############################################################################
# release ####################################################################
FROM rust:${TAG} as release
COPY --from=builder-release /railgun/target/release/railgun /bin/railgun
CMD ["/bin/railgun"]
