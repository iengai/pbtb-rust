# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

PBTB-Rust is a Telegram bot for managing Passivbot trading bot configurations. Built with Rust using Clean Architecture (DDD) with AWS services integration.

## Architecture

Dependencies flow inward: Interface → Use Case → Domain ← Infrastructure

- **Domain Layer** (`src/domain/`) - Core entities and repository traits (no external dependencies)
- **Use Case Layer** (`src/usecase/`) - Business logic orchestration, depends only on Domain traits
- **Infrastructure Layer** (`src/infra/`) - Implements Domain traits with AWS SDKs (DynamoDB, S3, ECS)
- **Interface Layer** (`src/interface/`) - Telegram handlers, Lambda handlers

Composition root is in `src/main.rs` where concrete implementations are injected into use cases via Arc.

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
cargo test                           # Run all tests
cargo test test_name                 # Run specific test
cargo test -- --nocapture            # With output

# Run
cargo run
```

## Testing

- Integration tests in `tests/` use DynamoDB Local at `http://dynamodb-local:8000`
- Inside Dev Container: tests work automatically
- Outside Dev Container: run `docker compose -f .devcontainer/docker-compose.yaml up -d dynamodb-local` first

## Code Style

- Rust 2024 edition
- Use `anyhow::Result` for application code, `thiserror` for domain-specific errors
- Avoid `unwrap()` and `expect()` - use `?` operator
- Use `async-trait` for async trait definitions

## Configuration

Layered config (priority low to high):
1. `config/default.toml` - Base settings
2. `config/{RUN_MODE}.toml` - Environment-specific
3. `config/local.toml` - Local overrides (gitignored)
4. Environment variables with `APP__` prefix (e.g., `APP__DYNAMODB__ENDPOINT_URL`)

Key env vars: `TELOXIDE_TOKEN` (Telegram bot token), `RUST_LOG` (log level)

## Data Storage

**DynamoDB** (bot metadata): PK = `user_id#{user_id}`, SK = `{bot_id}`

**S3** (configurations):
```
bucket/
├── predefined/           # Config templates
└── {user_id}/{bot_id}/   # User bot configs and API keys
```

## Infrastructure

Terraform modules in `terraform/modules/` (network, ecs, s3, dynamodb, task-definitions, lambda).
Deploy via `terraform/envs/dev/`.

## Git Workflow

- Run `cargo fmt && cargo clippy` before committing
- Commit messages: lowercase, descriptive (e.g., "add user authentication")

## Do Not

- Don't use `panic!`, `unwrap()`, or `expect()` in production code
- Don't commit `.env` files or API keys
- Don't skip clippy warnings
