# AGENTS.md

Project guidance for AI agents (Codex, Claude Code, etc.) working in this repository.

## Quick Context
- PBTB-Rust is a Telegram bot for managing Passivbot trading bot configurations.
- Architecture: Interface → Use Case → Domain ← Infrastructure (Clean Architecture / DDD).

## Development Environment

**Develop inside the Dev Container (`.devcontainer/`) — do not configure a host toolchain.** It pins the Rust toolchain and AWS CLI (via Dev Container features) and supplies the native build deps `aws-lc-sys` needs (NASM/cmake, commonly missing on Windows), plus DynamoDB Local for integration tests. Open the repo with the Dev Containers extension and "Reopen in Container"; the running service is `app-node` (e.g. `docker exec -it app-node bash`). Run all `cargo build` / `test` / `clippy` there rather than on the host.

Git worktrees: the container bind-mounts the folder it was opened on (`${localWorkspaceFolder}` → `/app`), so a session started in a worktree under a different path is **not** what the container builds. Reopen the container on the worktree, or run the build against the worktree path explicitly.

## Repo Layout (key paths)
- `src/domain/` core entities + repository traits (no external deps)
- `src/usecase/` business logic orchestrations
- `src/infra/` AWS implementations (DynamoDB, S3, ECS)
- `src/interface/telegram/` Telegram handlers
- `src/bin/task_state_change_handler/` AWS Lambda for ECS task-state events (observed-state sync + auto-restart)
- `config/` layered config (default + env overrides)
- `tests/` integration tests (DynamoDB Local via `testcontainers`)
- `terraform/` AWS IaC modules — deploy via `terraform/envs/dev/`
- `.devcontainer/` Dev Container setup for local dev

## Architecture Detail

Composition root is `src/main.rs`: concrete infra implementations are constructed and injected into use cases via `Arc`, then passed as a `Deps` struct to the Telegram interface layer.

### Binaries

- **`src/main.rs`** — Telegram bot (long-polling via teloxide)
- **`src/bin/task_state_change_handler/`** — AWS Lambda that listens to ECS Task State Change events (RUNNING + STOPPED) via EventBridge. On RUNNING it records observed-running via `RecordRunningTaskUseCase`; on STOPPED it parses the stop reason into a `StopInfo` and delegates the restart-or-skip decision to `ReconcileStoppedTaskUseCase`. Together they keep observed `BotRuntime` state in sync with reality, event by event

### Telegram Handler Routing

Three ordered branches in `src/interface/telegram/router.rs`:

1. **commands** — `/start`, etc. (BotCommand enum)
2. **callbacks** — inline keyboard button presses
3. **dialogue** — stateful multi-step flows (add bot, delete bot, set risk level)

State uses two in-memory stores: `DialogueState` (current flow step) and `BotContext` (currently selected bot ID).

### Desired State vs Observed State

The model deliberately separates two concepts that used to be conflated:

- **Desired state** = user intent. `Bot.enabled` (bool) records whether the user turned the bot on, toggled via `Bot::enable`/`Bot::disable`. The Telegram "Run bot"/"Stop bot" buttons drive `StartBotUseCase`/`StopBotUseCase`, which flip desired state **and** actuate ECS — `StartBotUseCase` launches the task (`RunTask`) behind the exclusive start lock, `StopBotUseCase` stops it (`StopTask`).
- **Observed state** = reality. The `BotRuntime` aggregate (`src/domain/runtime.rs`) records whether the ECS task is actually running (`RuntimePhase::{Starting,Running,Stopped}`, plus `task_id`, `version`, `observed_at`). `Running`/`Stopped` are written by the ECS Task State Change Lambda (`RecordRunningTaskUseCase` on RUNNING, `ReconcileStoppedTaskUseCase` on STOPPED); `Starting` is the transient lock state a launcher sets between claiming the start and the RUNNING event. Read via `GetBotRuntimeUseCase`.

The old `Bot.status` field and its `Status` enum were removed — they mixed the two concepts and were never persisted correctly.

### Auto-restart Reconciliation

`ReconcileStoppedTaskUseCase` owns the restart policy: it restarts a stopped task only when `enabled == true` (desired state ON) **and** the stop was memory-related (exit code 137 and not `UserInitiated`). The restart is claimed through the exclusive start lock (below), keyed on the stopped task id, so a duplicate/late STOPPED event cannot spawn a second task — it returns `SkippedSuperseded` in that case. Outcomes: `Restarted { task_id }`, `SkippedNotEnabled`, `SkippedNotMemoryRelated`, `SkippedSuperseded`, `BotNotFound`. This also fixes a prior bug where the Lambda restarted on OOM without checking `enabled`, resurrecting bots the user had manually disabled.

### Exclusive Start Lock (no double-run)

A bot must **never** run two live-trading tasks at once. Every launcher — the telebot "Run bot" and the Lambda auto-restart — claims an exclusive lock before `RunTask` via `StartLockRepository`, a `starting` row guarded by a DynamoDB **conditional write** (the write, not the read, is the authoritative gate):

- `try_acquire_start` (cold start) claims from `stopped`/absent, or from a `starting` lock older than `START_LOCK_STALE_AFTER_SECS`. Before a stale reclaim of a lock that still carries a `task_id`, `StartBotUseCase` confirms via ECS `DescribeTasks` (`TaskController::liveness`) that the task is actually gone, so a live task whose RUNNING event was lost is never double-launched.
- `try_acquire_restart` (Lambda) claims only while the stopped task is still the row's current `task_id`, making duplicate STOPPED events idempotent.

After winning the lock the launcher calls `attach_started_task` (records the new `task_id`) on success or `release_start` on launch failure; the Lambda's RUNNING event then flips `starting → running`.

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

## Security

- **`user_id` is the tenant isolation boundary.** Every row lives under `pk = user_id#{user_id}`, so a caller must only ever touch their own data. Derive the `pk` from the authenticated Telegram `user_id`, never from client-supplied input, and validate it before any read/write. Treat any cross-`user_id` access as a privilege-escalation bug.
- **Never log or surface credentials.** `api_key` / `secret_key` must not appear in logs, traces, or error messages shown to the user.
- Generic checks (IAM least-privilege, dependency CVEs via `cargo audit`, injection / path traversal) are not project-specific — run `/security-review` for those instead of expanding this section.

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
- **Comments describe the code as it is, for a reader who never saw the diff.** Do not narrate the change or the act of writing it: no "previously/now/no longer", "not just the first", "this replaces…", and do not frame new code by its pairing ("the counterpart to X", "together they…"). That is commit-message material. Keep comments for the non-obvious *why* — invariants, gotchas, ordering rules, external constraints — and cut anything that merely restates the code or only parses if you watched it being written. The `comment-reviewer` agent enforces this on the diff.

## Git Workflow
- Run `cargo fmt && cargo clippy` before committing

### Branch Naming

Use `<type>/<kebab-summary>`, where `<type>` is the same set as commit types
(`feat`, `fix`, `refactor`, `test`, `chore`, `docs`). Examples:
`fix/devcontainer-bind-mount`, `refactor/rich-domain-and-status-split`,
`chore/review-agents`.

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
