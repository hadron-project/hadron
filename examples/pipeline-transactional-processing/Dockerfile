## Dockerfile for pipeline-transactional-processing (txp) demo app.

# syntax=docker/dockerfile:1.3
ARG RUST_TAG=1.56.1
ARG ALPINE_TAG=3.14
FROM rust:${RUST_TAG}-alpine${ALPINE_TAG} as builder

LABEL maintainer="Anthony Dodd <dodd.anthonyjosiah@gmail.com>"
LABEL org.opencontainers.image.source https://github.com/hadron-project/hadron/
ENV CARGO_HOME=/pipeline-txp/target/.cargo
WORKDIR /pipeline-txp

RUN apk --no-cache add musl-dev

COPY ./Cargo.toml Cargo.toml
COPY ./Cargo.lock Cargo.lock
COPY ./src src
COPY ./migrations migrations
COPY ./sqlx-data.json sqlx-data.json

# Should only ever be "" or "--release".
ARG RELEASE_OPT=""
RUN --mount=type=cache,id=pipeline-txp,target=/pipeline-txp/target \
    cargo build ${RELEASE_OPT} \
    && if [ "${RELEASE_OPT}" == '' ]; then \
    cp /pipeline-txp/target/debug/pipeline-txp /bin/pipeline-txp; \
    else \
    cp /pipeline-txp/target/release/pipeline-txp /bin/pipeline-txp; \
    fi

##############################################################################
##############################################################################
FROM alpine:${ALPINE_TAG} as release
COPY --from=builder /bin/pipeline-txp /bin/pipeline-txp
CMD ["/bin/pipeline-txp"]
