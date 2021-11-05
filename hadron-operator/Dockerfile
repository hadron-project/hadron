## Dockerfile for hadron-operator.
##
## NOTE WELL: this Dockerfile assumes a context from the root of this repo.

# syntax=docker/dockerfile:1.3
ARG RUST_TAG=1.56.1
ARG ALPINE_TAG=3.14
FROM rust:${RUST_TAG}-alpine${ALPINE_TAG} as builder

LABEL maintainer="Anthony Dodd <dodd.anthonyjosiah@gmail.com>"
LABEL org.opencontainers.image.source https://github.com/hadron-project/hadron/
ENV CARGO_HOME=/hadron/hadron-operator/target/.cargo
WORKDIR /hadron

RUN apk --no-cache add musl-dev protoc && \
    rustup component add rustfmt

COPY ./hadron-operator/src hadron-operator/src
COPY ./hadron-operator/build.rs hadron-operator/build.rs
COPY ./hadron-operator/Cargo.toml hadron-operator/Cargo.toml
COPY ./hadron-operator/Cargo.lock hadron-operator/Cargo.lock
COPY ./proto/operator.proto proto/operator.proto
COPY ./hadron-core/src hadron-core/src
COPY ./hadron-core/Cargo.lock hadron-core/Cargo.lock
COPY ./hadron-core/Cargo.toml hadron-core/Cargo.toml

# Should only ever be "" or "--release".
ARG RELEASE_OPT=""
RUN --mount=type=cache,id=hadron-operator,target=/hadron/hadron-operator/target \
    cargo build --manifest-path hadron-operator/Cargo.toml ${RELEASE_OPT} \
    && if [ "${RELEASE_OPT}" == '' ]; then \
    cp /hadron/hadron-operator/target/debug/hadron-operator /bin/hadron-operator; \
    else \
    cp /hadron/hadron-operator/target/release/hadron-operator /bin/hadron-operator; \
    fi

##############################################################################
##############################################################################
FROM alpine:${ALPINE_TAG} as release
COPY --from=builder /bin/hadron-operator /bin/hadron-operator
CMD ["/bin/hadron-operator"]
