ARG TAG=1.37.0-slim-stretch
FROM rust:${TAG} as base

LABEL maintainer="Anthony Dodd"
WORKDIR /railgun

# Force a registry update.
# RUN cargo search --limit 1 --quiet # TODO: rm this. Only used where no network was available.

FROM base as builder-release
COPY ./src src
COPY ./Cargo.lock Cargo.lock
COPY ./Cargo.toml Cargo.toml
# COPY ./.cargo .cargo # TODO: rm this. Only used where no network was available.
# COPY ./vendor vendor # TODO: rm this. Only used where no network was available.
# COPY ./actix-raft actix-raft # TODO: rm this. Only used where no network was available.
RUN cargo build --release

FROM rust:${TAG} as release
COPY --from=builder-release /railgun/target/release/railgun /bin/railgun
CMD ["/bin/railgun"]
