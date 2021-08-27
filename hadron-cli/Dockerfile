## Dockerfile for hadron-cli.
##
## NOTE WELL: this Dockerfile assumes a context from the root of this repo.

# syntax=docker/dockerfile:1.3
ARG RUST_TAG=1.54.0
ARG ALPINE_TAG=3.14
FROM rust:${RUST_TAG}-alpine${ALPINE_TAG} as builder

LABEL maintainer="Anthony Dodd <dodd.anthonyjosiah@gmail.com>"
ENV CARGO_HOME=/hadron/hadron-cli/target/.cargo
WORKDIR /hadron

RUN apk --no-cache add musl-dev protoc && \
    rustup component add rustfmt

COPY ./hadron-cli/src hadron-cli/src
COPY ./hadron-cli/Cargo.toml hadron-cli/Cargo.toml
COPY ./hadron-cli/Cargo.lock hadron-cli/Cargo.lock
COPY ./proto/stream.proto proto/stream.proto
COPY ./proto/operator.proto proto/operator.proto
COPY ./hadron-client/src hadron-client/src
COPY ./hadron-client/build.rs hadron-client/build.rs
COPY ./hadron-client/Cargo.lock hadron-client/Cargo.lock
COPY ./hadron-client/Cargo.toml hadron-client/Cargo.toml

# Should only ever be "" or "--release".
ARG RELEASE_OPT=""
RUN --mount=type=cache,id=hadron-cli,target=/hadron/hadron-cli/target \
    cargo build --manifest-path hadron-cli/Cargo.toml ${RELEASE_OPT} \
    && if [ "${RELEASE_OPT}" == '' ]; then \
    cp /hadron/hadron-cli/target/debug/hadron /bin/hadron; \
    else \
    cp /hadron/hadron-cli/target/release/hadron /bin/hadron; \
    fi

##############################################################################
##############################################################################
FROM alpine:${ALPINE_TAG} as release

RUN apk update && apk upgrade && apk add --no-cache bash
COPY --from=builder /bin/hadron /bin/hadron
RUN hadron --help > /root/motd && \
    echo '' >> /root/motd && \
    echo 'Welcome! The Hadron CLI is available at /bin/hadron or simply as `hadron`.' >> /root/motd && \
    echo 'sleep 1; echo -n "."; sleep 1; echo -n "."; sleep 1; echo "."; cat /root/motd' > /root/.bashrc
CMD ["bash"]
