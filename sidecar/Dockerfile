# syntax=docker/dockerfile:1
FROM rust:1 AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/src/target \
    cargo build --release && cp target/release/nym-sidecar /nym-sidecar

FROM debian:trixie-slim
RUN apt-get update && apt-get install -y --no-install-recommends iptables ca-certificates curl \
 && rm -rf /var/lib/apt/lists/*
COPY --from=build /nym-sidecar /usr/local/bin/nym-sidecar
COPY entrypoint.sh /entrypoint.sh
ENTRYPOINT ["/entrypoint.sh"]
