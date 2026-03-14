# AGENTS.md

Project guidance for AI agents (Codex, Claude Code, etc.) working in this repository.

## Quick Context
- PBTB-Rust is a Telegram bot for managing Passivbot trading bot configurations.
- Architecture: Interface → Use Case → Domain ← Infrastructure (Clean Architecture / DDD).

## Repo Layout (key paths)
- `src/domain/` core entities + repository traits (no external deps)
- `src/usecase/` business logic orchestrations
- `src/infra/` AWS implementations (DynamoDB, S3, ECS)
- `src/interface/telegram/` Telegram handlers
- `src/bin/task_stopped_event_handler/` AWS Lambda for ECS task auto-restart
- `config/` layered config (default + env overrides)
- `tests/` integration tests (DynamoDB Local)
- `terraform/` AWS IaC modules — deploy via `terraform/envs/dev/`
- `.devcontainer/` Dev Container setup for local dev

## Architecture Detail

Composition root is `src/main.rs`: concrete infra implementations are constructed and injected into use cases via `Arc`, then passed as a `Deps` struct to the Telegram interface layer.

### Binaries

- **`src/main.rs`** — Telegram bot (long-polling via teloxide)
- **`src/bin/task_stopped_event_handler/`** — AWS Lambda that listens to ECS Task State Change events via EventBridge and auto-restarts Passivbot tasks that were OOM-killed (exit code 137)

### Telegram Handler Routing

Three ordered branches in `src/interface/telegram/router.rs`:

1. **commands** — `/start`, etc. (BotCommand enum)
2. **callbacks** — inline keyboard button presses
3. **dialogue** — stateful multi-step flows (add bot, delete bot, set risk level)

State uses two in-memory stores: `DialogueState` (current flow step) and `BotContext` (currently selected bot ID).

## Data Storage

**DynamoDB** (bot metadata): PK = `user_id#{user_id}`, SK = `{bot_id}`

**S3** (configurations):
```
bucket/
├── predefined/           # Config templates
└── {user_id}/{bot_id}/   # User bot configs and API keys
```

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
- Use `async-trait` for async trait definitions
- Avoid `panic!`, `unwrap()`, `expect()`; use `?` + context
- Keep domain layer free of external dependencies

## Git Workflow
- Run `cargo fmt && cargo clippy` before committing

### Commit Message Format

```
<type>: <short summary>

[optional body]
```

**Types:**
- `feat` — new feature
- `fix` — bug fix
- `refactor` — code change that neither fixes a bug nor adds a feature
- `test` — adding or updating tests
- `chore` — build, config, dependency updates
- `docs` — documentation only

**Rules:**
- Summary line: lowercase, imperative mood, no period, ≤72 chars
- Body: explain *why*, not *what* (the diff shows what)
- Reference issues with `closes #123` or `refs #123` in the body

**Examples:**
```
feat: add risk level update via telegram dialogue

fix: handle missing bot_id in ecs task stopped event

refactor: extract bot selection logic into BotContext helper
```

## Do Not
- Do not commit `.env` files or secrets
- Do not skip clippy warnings
- Do not introduce hardcoded credentials

## AI Agent Expectations
- Keep changes minimal and targeted
- Avoid scanning unrelated directories
- Ask before running long or destructive commands
- When changing behavior, add or update tests
