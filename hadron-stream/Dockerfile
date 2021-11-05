## Dockerfile for hadron-stream.
##
## NOTE WELL: this Dockerfile assumes a context from the root of this repo.

# syntax=docker/dockerfile:1.3
ARG RUST_TAG=1.56.1
ARG ALPINE_TAG=3.14
FROM rust:${RUST_TAG}-alpine${ALPINE_TAG} as builder

LABEL maintainer="Anthony Dodd <dodd.anthonyjosiah@gmail.com>"
LABEL org.opencontainers.image.source https://github.com/hadron-project/hadron/
ENV CARGO_HOME=/hadron/hadron-stream/target/.cargo
WORKDIR /hadron

RUN apk --no-cache add musl-dev protoc && \
    rustup component add rustfmt

COPY ./hadron-stream/proto hadron-stream/proto
COPY ./hadron-stream/src hadron-stream/src
COPY ./hadron-stream/build.rs hadron-stream/build.rs
COPY ./hadron-stream/Cargo.toml hadron-stream/Cargo.toml
COPY ./hadron-stream/Cargo.lock hadron-stream/Cargo.lock
COPY ./proto/stream.proto proto/stream.proto
COPY ./hadron-core/src hadron-core/src
COPY ./hadron-core/Cargo.lock hadron-core/Cargo.lock
COPY ./hadron-core/Cargo.toml hadron-core/Cargo.toml

# Should only ever be "" or "--release".
ARG RELEASE_OPT=""
RUN --mount=type=cache,id=hadron-stream,target=/hadron/hadron-stream/target \
    cargo build --manifest-path hadron-stream/Cargo.toml ${RELEASE_OPT} \
    && if [ "${RELEASE_OPT}" == '' ]; then \
    cp /hadron/hadron-stream/target/debug/hadron-stream /bin/hadron-stream; \
    else \
    cp /hadron/hadron-stream/target/release/hadron-stream /bin/hadron-stream; \
    fi

##############################################################################
##############################################################################
FROM alpine:${ALPINE_TAG} as release
COPY --from=builder /bin/hadron-stream /bin/hadron-stream
CMD ["/bin/hadron-stream"]
