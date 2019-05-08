FROM rust:1.34.1-slim as base

LABEL maintainer="Anthony Dodd"
WORKDIR /railgun

# Force a registry update.
RUN cargo search --limit 1 --quiet

FROM base as release
COPY ./src src
COPY ./Cargo.lock Cargo.lock
COPY ./Cargo.toml Cargo.toml
RUN cargo build --release

CMD ["/railgun/target/release/railgun"]
