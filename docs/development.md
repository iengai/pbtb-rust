# Local Development

The Dev Container (`.devcontainer/`) is the canonical build/test environment. It pins the Rust toolchain and AWS CLI (via Dev Container features) and supplies the native build dependencies `aws-lc-sys` needs — NASM and cmake, commonly missing on Windows hosts. It also brings up DynamoDB Local on the same Docker network for integration tests. Run all `cargo build` / `cargo test` / `cargo clippy` inside the container rather than on the host.

## Prerequisites

- Docker and Docker Compose
- (Optional) Rust toolchain on the host if you do not use Dev Containers
- AWS CLI (for local DynamoDB interaction)

## Open the Dev Container

The setup provides a consistent Rust environment out of the box and runs DynamoDB Local on the same Docker network.

- Entry file: `.devcontainer/devcontainer.json`
- Dependent services: `.devcontainer/docker-compose.yaml` (contains `dynamodb-local` and `app-node`)
- Build image: `.devcontainer/Dockerfile` (multi-stage — a `builder`/`runtime` pair for the production image, plus a lightweight `devcontainer` stage that the Dev Container builds via `target: devcontainer`; the Rust toolchain is installed on top by the `rust` feature)

### VS Code + Dev Containers extension

1. Install the extension `ms-vscode-remote.remote-containers`.
2. From the project root, run **Dev Containers: Reopen in Container**.
3. On the first build it will:
   - Start both `dynamodb-local` and `app-node` services
   - Bind-mount your source code into the container at `/app`
   - Install Rust, clippy, rustfmt, and prefetch dependencies (see `postCreateCommand`)

### JetBrains RustRover / IntelliJ

1. Install Gateway or the official Dev Containers plugin.
2. Choose **Open using devcontainer** on the project root.

### The running service

`app-node` is the development container (the running service). Attach a shell with:

```bash
docker exec -it app-node bash
```

### Networking and caching

- The app connects to DynamoDB Local by compose service name: `http://dynamodb-local:8000`. The same endpoint is injected as `APP__DYNAMODB__ENDPOINT_URL` on the `app-node` service in `docker-compose.yaml`.
- Named volumes (`pbtb-rust-cargo-registry`, `pbtb-rust-cargo-git`, `pbtb-rust-cargo-target`) cache the Cargo registry/git and the build target on native volume speed instead of the bind-mounted Windows source, keeping rebuilds fast. They match `CARGO_HOME=/usr/local/cargo` and `CARGO_TARGET_DIR=/app/target` set in `devcontainer.json`.
- `remoteUser: vscode` keeps UID/GID consistent with the host and avoids file-permission issues (the `vscode` user is created by the common-utils feature).
- Windows: ensure your drive is shared in Docker Desktop; for performance, consider the WSL2 backend.

### Troubleshooting

- **Slow builds:** verify named volumes exist (`docker volume ls` shows `pbtb-rust-cargo-*`) and your network/proxy is configured.
- **Permission issues:** confirm the container user is `vscode` (`whoami`) and check host file permissions; recreate the container if needed.
- **Cannot reach DynamoDB Local:** check container network and port usage, or access it from the host at `http://localhost:8000`. For data migration, see the `.devcontainer/docker/dynamodb` directory.

## Worktree / bind-mount caveat

The container bind-mounts the folder it was opened on (`${localWorkspaceFolder}` → `/app`). A session started in a Git worktree under a different path is **not** what the container builds. Reopen the container on the worktree, or run the build against the worktree path explicitly.

## Without the Dev Container (optional)

The host needs the native toolchain — `aws-lc-sys` requires NASM/cmake, often missing on Windows. The Dev Container is recommended for this reason.

- Run the test suite. The integration tests start their own `amazon/dynamodb-local` via `testcontainers`, so only Docker needs to be available:

  ```bash
  cargo test
  ```

- To run the bot against a local DynamoDB instead of testcontainers, start one yourself:

  ```bash
  docker compose -f .devcontainer/docker-compose.yaml up -d dynamodb-local
  ```

## Commands (inside the Dev Container)

```bash
# Build / check
cargo build
cargo check

# Lint
cargo clippy --all-targets --all-features -- -D warnings

# Format
cargo fmt --all
cargo fmt --all -- --check

# Test
cargo test
cargo test test_name
cargo test -- --nocapture
```

Interact with local DynamoDB from inside the container:

```bash
aws dynamodb list-tables --endpoint-url http://dynamodb-local:8000
```

Run `cargo fmt && cargo clippy` before committing.

## Testing

- Repository read/write integration tests use the `testcontainers` crate to spin up `amazon/dynamodb-local` programmatically — no manually managed container needed. They **skip gracefully** when Docker is unavailable: the test prints a skip message and returns successfully, so `cargo test` stays green without Docker.
- Use-case unit tests use in-memory mock repositories, so they run anywhere with no external services.

## Configuration

Config is assembled in `src/config/configs.rs` (`load_config`), which adds sources in this order:

1. `config/default.toml`
2. `config/{RUN_MODE}.toml` (optional)
3. `config/local.toml` (optional, gitignored)
4. `APP__*` environment variables (prefix `APP`, `__` as the nesting separator — e.g. `APP__DYNAMODB__ENDPOINT_URL` maps to `[dynamodb] endpoint_url`)

The `config` crate gives **later-added sources higher priority**, so the effective precedence is, low → high:

`config/default.toml`  <  `config/{RUN_MODE}.toml`  <  `config/local.toml`  <  `APP__*` env

> ✅ **`APP__*` env is the highest-priority layer** — an env var overrides the matching value from any config file, the usual "env wins" convention. The deployed binaries rely on this: the telebot and Lambda images ship only the compiled binary (no `config/` directory), so the file sources are absent and `APP__*` env is the sole config source. From a checkout where `config/` is present (local dev), an `APP__*` override still wins over the file value.

`RUN_MODE` is read from `APP__RUN_MODE`, falling back to `RUN_MODE`, defaulting to `default`.

Example local override (`config/local.toml` — highest priority):

```toml
[dynamodb]
endpoint_url = "http://localhost:8000"
region = "us-east-1"
table_name = "bots"
```

Notes:

- `config/default.toml` omits `dynamodb.endpoint_url` (DynamoDB targets real AWS unless overridden) but does set `s3.endpoint_url = "http://localhost:9000"` plus placeholder `region` / `table_name` / `bucket_name` / ECS ARNs for local use.
- Hostnames are context-dependent. **Inside the Dev Container**, use the compose service name (`http://dynamodb-local:8000`, injected as `APP__DYNAMODB__ENDPOINT_URL`). **From the host**, use `http://localhost:8000`.

### Key environment variables

| Variable | Description |
|----------|-------------|
| `TELOXIDE_TOKEN` | Telegram Bot API token |
| `RUST_LOG` | Log level (e.g., `info`, `debug`) |
| `APP__DYNAMODB__ENDPOINT_URL` | DynamoDB endpoint override |
| `APP__S3__ENDPOINT_URL` | S3 endpoint override |
| `APP__RUN_MODE` / `RUN_MODE` | Selects `config/{RUN_MODE}.toml` |

Do not commit `.env` files, secrets, or hardcoded credentials.
