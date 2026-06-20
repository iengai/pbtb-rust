# syntax=docker/dockerfile:1.7-labs
######## Build ########
FROM rust:1.89-bullseye AS builder
RUN apt-get update && apt-get install -y --no-install-recommends pkg-config libssl-dev ca-certificates && rm -rf /var/lib/apt/lists/*
WORKDIR /app

# Which binary to compile and ship. Must name a target in Cargo.toml (the
# package defines a `pbtb-rust` bin and a `task_state_change_handler` bin).
ARG BIN_NAME=pbtb-rust

# Copy the whole source tree (.dockerignore keeps out target/, .git, .devcontainer).
COPY . .

# Build only the shipped binary, then copy it out of the target cache mount.
#
# We deliberately do NOT use a "stub every [[bin]] then prefetch deps" trick:
# it had to be hand-edited for every new bin/lib target (and silently broke the
# build with "couldn't read <path>" when it wasn't), and — since the stubs
# `use` nothing — it made cargo recompile the heavy aws-sdk/hyper crates a
# second time under the real feature set. The BuildKit cache mounts below
# already persist the cargo registry and the target/ dir across builds, so
# dependency compilation is cached incrementally from the *real* dependency
# graph, with no stubs to maintain.
#
# target/ is a cache mount (not part of the image), so the freshly built binary
# is copied into /out/bin/, which the runtime stage then COPYs from.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release --locked --bin "${BIN_NAME}" \
 && mkdir -p /out/bin \
 && cp "target/release/${BIN_NAME}" "/out/bin/${BIN_NAME}" \
 && ls -l /out/bin

######## Devcontainer ########
# Lightweight image for the dev container: just the OS + build dependencies.
# The Rust toolchain is installed by the devcontainers "rust" feature, the
# source is bind-mounted at runtime, and compilation happens inside the running
# container (into the cargo-target volume) — so we deliberately do NOT compile
# the project at image-build time here.
#
# cmake/golang-go/nasm build aws-lc-sys (pulled in by the AWS SDK's TLS stack);
# without them `cargo build`/`test` and the Lambda build fail in the container.
FROM debian:bullseye-slim AS devcontainer
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      pkg-config \
      libssl-dev \
      ca-certificates \
      curl \
      git \
      build-essential \
      cmake \
      golang-go \
      nasm \
 && rm -rf /var/lib/apt/lists/*
WORKDIR /app

######## Runtime ########
FROM debian:bullseye-slim AS runtime
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