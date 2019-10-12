ARG TAG=1.37.0-slim-stretch
FROM rust:${TAG} as base

LABEL maintainer="Anthony Dodd"
WORKDIR /railgun

##############################################################################
# builder-watch ##############################################################
FROM base as builder-watch
RUN cargo install cargo-watch --version 7.2.1

COPY ./src src
COPY ./Cargo.lock Cargo.lock
COPY ./Cargo.toml Cargo.toml
COPY ./build.rs build.rs
COPY ./protobuf protobuf
RUN cargo build --release

CMD ["cargo", "watch", "-i", "src/proto/*", "-s", "cargo build --release"]

##############################################################################
# release-watch ##############################################################
FROM rust:${TAG} as release-watch
COPY --from=builder-watch /railgun/target/release/railgun /bin/railgun
CMD ["/bin/railgun"]

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
