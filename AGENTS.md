# AGENTS.md

Project guidance for Codex (and other AI agents) working in this repository.

## Quick Context
- PBTB-Rust is a Telegram bot for managing Passivbot trading bot configurations.
- Architecture: Interface → Use Case → Domain ← Infrastructure (Clean Architecture / DDD).

## Repo Layout (key paths)
- `src/domain/` core entities + repository traits (no external deps)
- `src/usecase/` business logic orchestrations
- `src/infra/` AWS implementations (DynamoDB, S3, ECS)
- `src/interface/telegram/` Telegram handlers
- `config/` layered config (default + env overrides)
- `tests/` integration tests (DynamoDB Local)
- `terraform/` AWS IaC modules
- `.devcontainer/` Dev Container setup for local dev

## Commands (inside Dev Container)
```bash
# Build / Check / Lint
cargo build
cargo check
cargo clippy --all-targets --all-features -- -D warnings

# Format
cargo fmt --all
cargo fmt --all -- --check

# Test
cargo test
cargo test test_name
cargo test -- --nocapture
```

## Testing Notes
- Integration tests rely on DynamoDB Local at `http://dynamodb-local:8000`.
- Outside Dev Container: start DynamoDB via:
  `docker compose -f .devcontainer/docker-compose.yaml up -d dynamodb-local`

## Configuration
Layered config priority (low → high):
1) `config/default.toml`
2) `config/{RUN_MODE}.toml`
3) `config/local.toml` (gitignored)
4) Env vars `APP__*` (e.g., `APP__DYNAMODB__ENDPOINT_URL`)

Key env vars: `TELOXIDE_TOKEN`, `RUST_LOG`

## Code Style & Conventions
- Rust 2024 edition
- Prefer `anyhow::Result` in application code, `thiserror` for domain errors
- Avoid `panic!`, `unwrap()`, `expect()`; use `?` + context
- Keep domain layer free of external dependencies

## Do Not
- Do not commit `.env` files or secrets
- Do not skip clippy warnings
- Do not introduce hardcoded credentials

## AI Agent Expectations
- Keep changes minimal and targeted
- Avoid scanning unrelated directories
- Ask before running long or destructive commands
- When changing behavior, add or update tests

## Related Docs
- `.claude/CLAUDE.md` contains aligned guidance (same rules, Claude-specific)
