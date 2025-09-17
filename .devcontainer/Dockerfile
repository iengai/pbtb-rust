# syntax=docker/dockerfile:1.7-labs
######## Build ########
FROM rust:1.89-bullseye AS builder
RUN apt-get update && apt-get install -y --no-install-recommends pkg-config libssl-dev ca-certificates && rm -rf /var/lib/apt/lists/*
WORKDIR /app

# Configuration: binary name and (optional) workspace package path
ARG BIN_NAME=pbtb-rust
ARG PKG_PATH=.

COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src && echo 'fn main(){}' > src/main.rs
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release --locked
RUN rm -rf src
COPY . .

# Explicitly specify the build target (for workspaces, add -p)
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    sh -euxc '\
      CARGO=/usr/local/cargo/bin/cargo; \
      mkdir -p /out/bin; \
      if [ -n "${PKG_PATH:-}" ]; then \
        "$CARGO" install --locked --path "${PKG_PATH}" --bin "${BIN_NAME}" --root /out; \
      else \
        "$CARGO" install --locked --path . --bin "${BIN_NAME}" --root /out; \
      fi; \
      ls -l /out/bin \
    '

######## Runtime ########
FROM debian:bullseye-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      ca-certificates \
      pkg-config \
      libssl-dev \
      build-essential \
 && update-ca-certificates \
 && rm -rf /var/lib/apt/lists/*
RUN useradd -m -u 10001 appuser
WORKDIR /app

ARG BIN_NAME=pbtb-rust

# Copy from the fixed output directory instead of relying on target/release
COPY --from=builder /out/bin/${BIN_NAME} /app/${BIN_NAME}

USER appuser
CMD ["/app/pbtb-rust"]