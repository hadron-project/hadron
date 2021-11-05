## Dockerfile for stream-transactional-processing (txp) demo app.

# syntax=docker/dockerfile:1.3
ARG RUST_TAG=1.56.1
ARG ALPINE_TAG=3.14
FROM rust:${RUST_TAG}-alpine${ALPINE_TAG} as builder

LABEL maintainer="Anthony Dodd <dodd.anthonyjosiah@gmail.com>"
LABEL org.opencontainers.image.source https://github.com/hadron-project/hadron/
ENV CARGO_HOME=/stream-txp/target/.cargo
WORKDIR /stream-txp

RUN apk --no-cache add musl-dev

COPY ./Cargo.toml Cargo.toml
COPY ./Cargo.lock Cargo.lock
COPY ./src src
COPY ./migrations migrations
COPY ./sqlx-data.json sqlx-data.json

# Should only ever be "" or "--release".
ARG RELEASE_OPT=""
RUN --mount=type=cache,id=stream-txp,target=/stream-txp/target \
    cargo build ${RELEASE_OPT} \
    && if [ "${RELEASE_OPT}" == '' ]; then \
    cp /stream-txp/target/debug/stream-txp /bin/stream-txp; \
    else \
    cp /stream-txp/target/release/stream-txp /bin/stream-txp; \
    fi

##############################################################################
##############################################################################
FROM alpine:${ALPINE_TAG} as release
COPY --from=builder /bin/stream-txp /bin/stream-txp
CMD ["/bin/stream-txp"]
