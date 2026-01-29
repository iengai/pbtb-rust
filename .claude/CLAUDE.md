# PBTB-Rust Project Guidelines

## Project Overview

This is a Rust-based trading bot management system using Clean Architecture (DDD) with Telegram interface and AWS services.

## Architecture

- **Domain Layer** (`src/domain/`) - Core entities and repository traits (no dependencies)
- **Use Case Layer** (`src/usecase/`) - Business logic, depends on Domain traits
- **Infrastructure Layer** (`src/infra/`) - Implements Domain traits with AWS services
- **Interface Layer** (`src/interface/`) - Telegram handlers, Lambda handlers

Dependencies flow inward: Interface → Use Case → Domain ← Infrastructure

## Code Style

- Rust 2024 edition
- Use `anyhow::Result` for error handling in application code
- Use `thiserror` for domain-specific errors
- Avoid `unwrap()` and `expect()` - use proper error handling with `?` operator
- Use `async-trait` for async trait definitions
- Follow repository trait pattern for data access

## Commands

```bash
# Build
cargo build
cargo build --release

# Check and Lint
cargo check
cargo clippy --all-targets --all-features -- -D warnings

# Format
cargo fmt --all
cargo fmt --all -- --check  # Check only

# Test
cargo test
cargo test -- --nocapture  # With output

# Run
cargo run
```

## Testing

- Integration tests are in `tests/` directory
- Tests use DynamoDB Local (endpoint: `http://dynamodb-local:8000`)
- Run `docker compose -f .devcontainer/docker-compose.yaml up -d dynamodb-local` before testing outside container

## Git Workflow

- Use feature branches
- Run `cargo fmt && cargo clippy` before committing
- Commit messages: lowercase, descriptive (e.g., "add user authentication")
- Squash commits when merging

## AWS Services

| Service | Purpose | Config Key |
|---------|---------|------------|
| DynamoDB | Bot metadata storage | `dynamodb.*` |
| S3 | Configuration and API keys | `s3.*` |
| ECS | Bot container execution | `ecs.*` |
| Lambda | Event handlers | - |

## Configuration

- Config files: `config/default.toml`, `config/{RUN_MODE}.toml`, `config/local.toml`
- Environment override prefix: `APP__` (e.g., `APP__DYNAMODB__ENDPOINT_URL`)
- Telegram token: `TELOXIDE_TOKEN` environment variable

## Important Patterns

### Repository Pattern
```rust
// Domain defines trait
pub trait BotRepository: Send + Sync {
    async fn save(&self, bot: &Bot) -> Result<()>;
    async fn find(&self, user_id: &str, bot_id: &str) -> Result<Option<Bot>>;
}

// Infrastructure implements
pub struct DynamoDbBotRepository { /* ... */ }
impl BotRepository for DynamoDbBotRepository { /* ... */ }
```

### Use Case Pattern
```rust
pub struct AddBotUseCase<R: BotRepository> {
    repository: Arc<R>,
}

impl<R: BotRepository> AddBotUseCase<R> {
    pub async fn execute(&self, input: AddBotInput) -> Result<Bot> {
        // Business logic here
    }
}
```

## Do Not

- Don't use `panic!`, `unwrap()`, or `expect()` in production code
- Don't commit `.env` files or API keys
- Don't modify `config/local.toml` (gitignored)
- Don't skip clippy warnings
