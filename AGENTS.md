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
- `src/bin/task_stopped_event_handler/` AWS Lambda for ECS task reconciliation / auto-restart
- `config/` layered config (default + env overrides)
- `tests/` integration tests (DynamoDB Local via `testcontainers`)
- `terraform/` AWS IaC modules — deploy via `terraform/envs/dev/`
- `.devcontainer/` Dev Container setup for local dev

## Architecture Detail

Composition root is `src/main.rs`: concrete infra implementations are constructed and injected into use cases via `Arc`, then passed as a `Deps` struct to the Telegram interface layer.

### Binaries

- **`src/main.rs`** — Telegram bot (long-polling via teloxide)
- **`src/bin/task_stopped_event_handler/`** — AWS Lambda that listens to ECS Task State Change events via EventBridge. It parses the stop reason into a `StopInfo` and delegates the restart-or-skip decision to `ReconcileStoppedTaskUseCase` (it no longer inlines the restart logic)

### Telegram Handler Routing

Three ordered branches in `src/interface/telegram/router.rs`:

1. **commands** — `/start`, etc. (BotCommand enum)
2. **callbacks** — inline keyboard button presses
3. **dialogue** — stateful multi-step flows (add bot, delete bot, set risk level)

State uses two in-memory stores: `DialogueState` (current flow step) and `BotContext` (currently selected bot ID).

### Desired State vs Observed State

The model deliberately separates two concepts that used to be conflated:

- **Desired state** = user intent. `Bot.enabled` (bool) records whether the user turned the bot on. It is toggled via `Bot::enable`/`Bot::disable` and the `SetBotEnabledUseCase`, which is wired to the Telegram "Run bot"/"Stop bot" buttons. These flip desired state only — they do **not** directly start or stop the ECS task.
- **Observed state** = reality. The `BotRuntime` aggregate (`src/domain/runtime.rs`) records whether the ECS task is actually running (`RuntimePhase::{Running,Stopped}`, plus `task_id`, `version`, `observed_at`). It is written by `ReconcileStoppedTaskUseCase` and read by `GetBotRuntimeUseCase`.

The old `Bot.status` field and its `Status` enum were removed — they mixed the two concepts and were never persisted correctly.

### Auto-restart Reconciliation

`ReconcileStoppedTaskUseCase` owns the restart policy: it restarts a stopped task only when `enabled == true` (desired state ON) **and** the stop was memory-related (exit code 137 and not `UserInitiated`). It returns one of `Restarted { task_id }`, `SkippedNotEnabled`, `SkippedNotMemoryRelated`, or `BotNotFound`, and records the resulting `BotRuntime` either way. This fixes a prior bug where the Lambda restarted on OOM without checking `enabled`, resurrecting bots the user had manually disabled.

## Data Storage

**DynamoDB** (single table, two row kinds under one `pk = user_id#{user_id}`):
- **Bot rows** — `sk = {bot_id}`. Attributes: `name`, `exchange`, `api_key`, `secret_key`, `enabled`, `created_at`, `updated_at` (no `status` attribute).
- **Runtime rows** — `sk = ecs_task_metadata#{bot_id}`. Observed `BotRuntime` for a bot: `status` (running/stopped), `task_id`, `task_updated_at`, `task_current_version`.

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
- The Dev Container is the canonical build/test environment. The host may lack the native toolchain (`aws-lc-sys` needs NASM/cmake, often missing on Windows), so prefer running `cargo build`/`cargo test`/clippy inside the container.
- Repository read/write integration tests use the `testcontainers` crate to spin up `amazon/dynamodb-local` programmatically (no manually-managed container needed). They **skip gracefully** when Docker is unavailable — the test prints a skip message and returns successfully, so `cargo test` stays green without Docker.
- Use-case unit tests use in-memory mock repositories, so they run anywhere with no external services.

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
- Domain fallibility uses the `DomainError` enum (`thiserror`), not `Result<_, String>`; the use-case layer still surfaces `String` to the interface
- Value objects validate on construction: `RiskLevel::new`/`Leverage::new` return `Result`, so any instance is guaranteed in-range
- Keep business rules inside the entity. `BotConfig` owns its invariants: `apply_risk_level` sets the risk and derives leverage (`= max(long, short) + 1`) atomically; `set_live_user` binds `live.user`; `from_template` is fallible and binds `live.user` on construction. Do not re-implement the leverage-derivation rule in the use-case layer.

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
