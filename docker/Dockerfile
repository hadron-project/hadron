# NOTE WELL: this Dockerfile assumes a context from the root of this repo.
#
# TODO: add these back in once https://github.com/clux/kube-rs/pull/376 lands.
#
# ARG RUST_TAG=1.53.0
# ARG ALPINE_TAG=3.12
# FROM rust:${RUST_TAG}-alpine${ALPINE_TAG} as builder
# RUN apk --no-cache add musl-dev
# FROM alpine:${ALPINE_TAG} as release
FROM rust:1.53-slim-buster as builder

LABEL maintainer="Anthony Dodd <dodd.anthonyjosiah@gmail.com>"
WORKDIR /hadron

##############################################################################
##############################################################################
FROM builder as builder.deps
RUN mkdir src && echo 'fn main() { }' > src/main.rs
RUN apt-get update -y && apt-get install protobuf-compiler libssl-dev pkg-config
COPY ./hadrond/Cargo.lock hadrond/Cargo.lock
COPY ./hadrond/Cargo.toml hadrond/Cargo.toml
COPY ./proto proto
RUN cargo build --manifest-path hadrond/Cargo.toml --release

##############################################################################
##############################################################################
FROM builder.deps as builder.app
COPY ./hadrond hadrond
RUN cargo build --manifest-path hadrond/Cargo.toml --release && mv hadrond/target/release/hadrond /bin/hadrond

##############################################################################
##############################################################################
FROM debian:buster-slim as release
RUN apt-get update -y && apt-get install libssl-dev pkg-config -y
COPY --from=builder.app /bin/hadrond /bin/hadrond
CMD ["/bin/hadrond"]
