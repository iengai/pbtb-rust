# PBTB-Rust

A Rust project with DynamoDB integration.

## Prerequisites

- Docker and Docker Compose
- (Optional) Rust toolchain on host if you don't use Dev Containers
- AWS CLI (for local DynamoDB interaction)

## Use Dev Containers (Recommended)

This project includes a .devcontainer setup that provides a consistent Rust development environment out of the box and runs DynamoDB Local on the same Docker network.

- Entry file: `.devcontainer/devcontainer.json`
- Dependent services: `.devcontainer/docker-compose.yaml` (contains `dynamodb-local` and `app-node`)
- Build image: `.devcontainer/Dockerfile` (used for production runtime image; the Dev Container installs the Rust toolchain on top)

### How to open

1) VS Code + Dev Containers extension
- Install extension: ms-vscode-remote.remote-containers
- From the project root, run: Dev Containers: Reopen in Container
- On the first build it will:
  - Start both `dynamodb-local` and `app-node` services
  - Bind-mount your source code to the container at `/app`
  - Install Rust, clippy, rustfmt, and prefetch dependencies (see `postCreateCommand`)

2) JetBrains RustRover / IntelliJ
- Install Gateway or use the official Dev Containers plugin
- Choose "Open using devcontainer" on the project root

### Common commands inside the container

- Build/check:
  - `cargo check`
  - `cargo build`
  - `cargo clippy --all-targets --all-features -- -D warnings`
  - `cargo fmt --all`
- Run tests: `cargo test`
- Interact with local DynamoDB (inside the container): `aws dynamodb list-tables --endpoint-url http://dynamodb-local:8000`

### Configuration and networking

- The app connects to DynamoDB Local via service name in the compose network: `http://dynamodb-local:8000`
- The same endpoint is also exposed as an env var in devcontainer.json: `APP__DYNAMODB__ENDPOINT_URL`
- Named volumes are used via `mounts` to cache Cargo registry/git and speed up builds
- `remoteUser: vscode` avoids host file permission issues (created by the common-utils feature)

### Best practices

- Cache Cargo data in named volumes: devcontainer.json configures `/home/vscode/.cargo/registry` and `/home/vscode/.cargo/git`, significantly speeding up rebuilds.
- Avoid writing as root: `remoteUser: vscode` keeps UID/GID consistent with the host and reduces permission issues.
- Bind mount only the source: workspace `/app` is bound to host code to avoid rebuilding the image unnecessarily.
- Use the same compose network for dependencies: containers communicate by service name (e.g., `dynamodb-local`).
- Prefetch tools and dependencies on first create: `postCreateCommand` installs clippy/rustfmt and runs `cargo fetch`.
- Use AWS CLI inside the container to avoid polluting the host environment.
- Windows tip: ensure your drive is shared in Docker Desktop; for performance, consider the WSL2 backend.

### Troubleshooting

- Slow builds: verify named volumes are created (`docker volume ls` shows devcontainer-cargo-*) and your network/proxy is configured properly.
- Permission issues: ensure the container user is `vscode` (`whoami`) and check host file permissions; recreate the container if needed.
- Cannot reach DynamoDB Local: check container network and port usage, or access it from the host at `http://localhost:8000`; for data migration, see the `docker/dynamodb` directory.

## Without Dev Containers (Optional)

1. Start local DynamoDB:
   ```
   docker compose -f .devcontainer/docker-compose.yaml up -d dynamodb-local
   ```
2. On the host, run:
   ```
   cargo test
   ```

## Configuration

The application uses a configuration file located at `config/default.toml`. You can override these settings by creating a `config/local.toml` file or by setting environment variables with the prefix `APP__`.

Example configuration:
```toml
[dynamodb]
endpoint_url = "http://localhost:8000"
region = "us-east-1"
table_name = "bots"
```

## Running Tests (in container)

- In the Dev Container terminal, run:
  ```
  cargo test
  ```
- The tests will create the required table(s) in the local DynamoDB instance.

## Project Structure

- `src/domain/bot.rs`: Domain model and repository trait for bots
- `src/infra/botrepository.rs`: DynamoDB implementation of the bot repository
- `src/infra/dynamodb/client.rs`: DynamoDB client creation, table ensure/setup utilities
- `src/config/configs.rs`: Configuration loading and environment overrides
- `src/config/dynamodb.rs`: DynamoDB configuration structure
- `config/`: TOML config files (default.toml, local overrides)
- `tests/`: Test files (integration-style tests using local DynamoDB)

## Development

To add a new feature:

1. Add or modify the domain model in `src/domain`
2. Implement or extend the repository in `src/infra`
3. Add tests under `tests`
4. Update `config/` or code under `src/config` if configuration changes are needed
