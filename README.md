# PBTB-Rust

A Telegram bot application written in Rust for managing Passivbot trading bot configurations. This project provides an interactive Telegram interface for creating, configuring, and managing automated cryptocurrency trading bots with full AWS infrastructure integration.

## Purpose

PBTB-Rust serves as a management layer for [Passivbot](https://github.com/enarjord/passivbot), enabling users to:

- **Bot Management**: Create, delete, and list trading bots through Telegram
- **Configuration Management**: Apply predefined configuration templates to bots
- **Risk Management**: Dynamically adjust risk levels (long/short position exposure)
- **API Key Management**: Securely store and manage exchange API credentials
- **Interactive Interface**: Full Telegram bot interface with inline keyboards and conversation flows

## Technology Stack

### Core Technologies

| Technology | Version | Purpose |
|------------|---------|---------|
| **Rust** | 2024 Edition | Primary programming language |
| **Tokio** | 1.x | Asynchronous runtime with full features |
| **Teloxide** | 0.12 | Telegram Bot API framework |

### AWS Services

| Service | SDK Version | Purpose |
|---------|-------------|---------|
| **DynamoDB** | aws-sdk-dynamodb 1.x | Bot metadata storage |
| **S3** | aws-sdk-s3 1.108.0 | Configuration and API key storage |
| **ECS** | - | Container orchestration (via Terraform) |

### Supporting Libraries

- **serde** / **serde_json** - Serialization framework
- **config** - Configuration file management
- **async-trait** - Async trait support
- **uuid** - Unique identifier generation
- **anyhow** - Error handling
- **env_logger** - Logging
- **lambda_runtime** - AWS Lambda support (optional)

### Infrastructure as Code

- **Terraform** - AWS infrastructure provisioning
- **Docker** - Containerization and local development

## Architecture

The project follows **Domain-Driven Design (DDD)** with **Clean Architecture** principles, applying the **Dependency Inversion Principle** where all layers depend on abstractions defined in the Domain layer.

```
┌─────────────────────────────────────────────────────────────────┐
│                       Interface Layer                           │
│              (Telegram Bot, Lambda Handlers)                    │
│                            │                                    │
│                            ▼ depends on                         │
│  ┌─────────────────────────────────────────────────────────┐    │
│  │                    Use Case Layer                       │    │
│  │       (AddBot, ListBots, RunTask, ApplyTemplate)        │    │
│  │                         │                               │    │
│  │                         ▼ depends on                    │    │
│  │  ┌───────────────────────────────────────────────────┐  │    │
│  │  │                  Domain Layer                     │  │    │
│  │  │      (Bot, BotConfig, ConfigTemplate entities)    │  │    │
│  │  │      (Repository Traits - abstractions)           │  │    │
│  │  └───────────────────────────────────────────────────┘  │    │
│  │                         ▲                               │    │
│  │                         │ implements                    │    │
│  │  ┌───────────────────────────────────────────────────┐  │    │
│  │  │              Infrastructure Layer                 │  │    │
│  │  │    (DynamoDB Repository, S3 Repository, ECS)      │  │    │
│  │  └───────────────────────────────────────────────────┘  │    │
│  └─────────────────────────────────────────────────────────┘    │
└─────────────────────────────────────────────────────────────────┘

Dependency Flow: Interface → Use Case → Domain ← Infrastructure
                 (Infrastructure implements Domain abstractions)
```

### Layer Responsibilities

| Layer | Description | Dependencies |
|-------|-------------|--------------|
| **Domain** | Core entities, repository traits (abstractions), business rules | None (innermost layer) |
| **Use Case** | Business logic orchestration, workflow coordination | Domain (traits) |
| **Infrastructure** | Implements repository traits with AWS services | Domain (traits) |
| **Interface** | Telegram handlers, Lambda handlers | Use Case, Domain |

### Dependency Injection

At runtime, concrete implementations from Infrastructure are injected into Use Cases via the composition root (`main.rs`):

```rust
// Domain: defines abstraction
pub trait BotRepository { async fn save(&self, bot: &Bot) -> Result<()>; }

// Infrastructure: implements abstraction
pub struct DynamoDbBotRepository { /* ... */ }
impl BotRepository for DynamoDbBotRepository { /* ... */ }

// Use Case: depends on abstraction
pub struct AddBotUseCase<R: BotRepository> { repository: R }

// Composition Root: wires concrete implementation
let repository = DynamoDbBotRepository::new(client);
let use_case = AddBotUseCase::new(repository);
```

### Layer Details

#### Domain Layer (`src/domain/`)
Core business entities and repository interfaces (no external dependencies):
- `Bot` - Trading bot aggregate root with metadata
- `BotConfig` - User-specific bot configuration
- `ConfigTemplate` - Reusable configuration templates
- `Exchange` - Supported exchanges (currently Bybit)
- Value Objects: `RiskLevel`, `Leverage`, `Coins`

#### Infrastructure Layer (`src/infra/`)
Concrete implementations of repository interfaces:
- `DynamoDbBotRepository` - Bot persistence in DynamoDB
- `S3BotConfigRepository` - Configuration storage in S3
- `S3ConfigTemplateRepository` - Template storage in S3
- `S3ApiKeyRepository` - Secure API key storage

#### Use Case Layer (`src/usecase/`)
Business logic orchestration:
- `AddBotUseCase` - Create new trading bot
- `DeleteBotUseCase` - Remove bot and associated data
- `ListBotsUseCase` - Retrieve user's bots
- `ListTemplatesUseCase` - List available templates
- `ApplyTemplateUseCase` - Apply template to bot
- `GetBotConfigUseCase` - Retrieve bot configuration
- `UpdateBotConfigUseCase` - Update full configuration
- `UpdateRiskLevelUseCase` - Adjust risk parameters

#### Interface Layer (`src/interface/telegram/`)
Telegram bot implementation:
- `router.rs` - Teloxide dispatcher setup
- `commands.rs` - Slash command handlers (`/start`, `/list`)
- `callbacks.rs` - Inline button handlers
- `dialogue.rs` - Conversation state management
- `keyboards.rs` - Menu and button layouts

## Project Structure

```
pbtb-rust/
├── src/
│   ├── main.rs                    # Application entry point
│   ├── lib.rs                     # Library exports
│   ├── config/                    # Configuration management
│   │   ├── configs.rs             # Config loading logic
│   │   ├── dynamodb.rs            # DynamoDB config struct
│   │   └── s3.rs                  # S3 config struct
│   ├── domain/                    # Domain models and traits
│   │   ├── bot.rs                 # Bot entity
│   │   ├── botconfig.rs           # BotConfig entity
│   │   ├── configtemplate.rs      # Template entity
│   │   └── exchange.rs            # Exchange enum
│   ├── infra/                     # Infrastructure implementations
│   │   ├── client.rs              # AWS client initialization
│   │   ├── botrepository.rs       # DynamoDB bot repository
│   │   ├── botconfigrepository.rs # S3 config repository
│   │   └── configtemplaterepository.rs
│   ├── usecase/                   # Business use cases
│   └── interface/
│       └── telegram/              # Telegram bot interface
├── config/
│   └── default.toml               # Default configuration
├── tests/                         # Integration tests
├── terraform/                     # AWS infrastructure
│   ├── envs/dev/                  # Environment configs
│   └── modules/                   # Reusable modules
│       ├── network/               # VPC, subnets, security groups
│       ├── ecs/                   # ECS cluster configuration
│       ├── s3/                    # S3 bucket configuration
│       └── task-definitions/      # ECS task definitions
└── .devcontainer/                 # Dev Container setup
    ├── devcontainer.json
    ├── docker-compose.yaml
    └── Dockerfile
```

## Data Storage Design

### DynamoDB Schema (Bots Table)

```
Primary Key: PK = "user_id#<user_id>", SK = "<bot_id>"
Attributes: name, exchange, api_key, secret_key, enabled, created_at, updated_at
```

### S3 Storage Structure

```
Bucket: {project}-{env}-bot-configs
├── predefined/              # Configuration templates
│   ├── template1.json
│   └── template2.json
└── {user_id}/              # User-specific data
    └── {bot_id}/
        ├── {bot_id}.json   # Bot configuration
        └── api-keys.json   # API credentials
```

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

The application uses a layered configuration system (priority from low to high):

1. `config/default.toml` - Default settings
2. `config/{RUN_MODE}.toml` - Environment-specific (optional)
3. `config/local.toml` - Local overrides (gitignored)
4. Environment variables `APP__*` - Runtime overrides

Example configuration:
```toml
[dynamodb]
endpoint_url = "http://localhost:8000"
region = "us-east-1"
table_name = "bots"

[s3]
endpoint_url = "http://localhost:9000"
region = "us-east-1"
bucket_name = "local-bot-configs"
```

### Environment Variables

| Variable | Description |
|----------|-------------|
| `TELOXIDE_TOKEN` | Telegram Bot API token |
| `RUST_LOG` | Log level (e.g., `info`, `debug`) |
| `APP__DYNAMODB__ENDPOINT_URL` | DynamoDB endpoint override |
| `APP__S3__ENDPOINT_URL` | S3 endpoint override |

## Running Tests

In the Dev Container terminal:
```
cargo test
```

The tests will create the required table(s) in the local DynamoDB instance.

## AWS Infrastructure (Terraform)

The `terraform/` directory contains infrastructure as code for deploying to AWS:

### Modules

- **network** - VPC, subnets (public/private), NAT gateway, security groups
- **ecs** - ECS cluster with EC2 capacity provider (ARM64 `t4g.medium` instances)
- **s3** - Bot configuration bucket with encryption and versioning
- **task-definitions** - ECS task definitions for Passivbot containers

### Deployment

```bash
cd terraform/envs/dev
terraform init
terraform plan
terraform apply
```

## Development

To add a new feature:

1. Add or modify the domain model in `src/domain`
2. Implement or extend the repository in `src/infra`
3. Create or update use cases in `src/usecase`
4. Add Telegram interface handlers in `src/interface/telegram`
5. Add tests under `tests`
6. Update `config/` or code under `src/config` if configuration changes are needed

## Security Considerations

- API keys are stored separately in S3 with encryption at rest
- S3 buckets block all public access
- IAM roles follow least-privilege principle
- Telegram bot token is loaded from environment variables only
