# NOTE WELL: this Dockerfile assumes a context from the root of this repo.
FROM rust:1.48.0

LABEL maintainer="Anthony Dodd <dodd.anthonyjosiah@gmail.com>"
WORKDIR /hadron

RUN apt-get update -y && apt-get install -y protobuf-compiler && \
    rustup component add rustfmt

# NOTE WELL: this is intended to be used with the docker-compose.yml file in the root
# of this repo, which provides volumes mounts and such to expedite this build.

CMD ["sh", "-c", "cargo build --manifest-path hadrond/Cargo.toml --release && mv hadrond/target/release/hadrond /.artifacts/hadrond"]
