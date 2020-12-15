# NOTE WELL: this Dockerfile assumes a context from the root of this repo.
ARG RUST_TAG=1.48.0
ARG ALPINE_TAG=3.12
FROM rust:${RUST_TAG} as builder

LABEL maintainer="Anthony Dodd <dodd.anthonyjosiah@gmail.com>"
WORKDIR /hadron

##############################################################################
##############################################################################
FROM builder as builder.os-deps
RUN apt-get update -y && apt-get install protobuf-compiler

##############################################################################
##############################################################################
FROM builder.os-deps as builder.cargo-registry
RUN cargo search cargo --quiet --limit 1

##############################################################################
##############################################################################
FROM builder.cargo-registry as builder.app
COPY ./hadrond hadrond
COPY ./protobuf protobuf
RUN cargo build --manifest-path hadrond/Cargo.toml --release && mv hadrond/target/release/hadrond /bin/hadrond

##############################################################################
##############################################################################
FROM alpine:${ALPINE_TAG} as release
COPY --from=builder.app /bin/hadrond /bin/hadrond
CMD ["/bin/hadrond"]
