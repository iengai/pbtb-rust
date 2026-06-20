# Architecture

PBTB-Rust follows **Domain-Driven Design (DDD)** with **Clean Architecture**, applying the **Dependency Inversion Principle**: every layer depends on abstractions (traits) defined in the Domain layer, so the dependency arrows all point inward toward the Domain.

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

## Layer Responsibilities

| Layer | Description | Dependencies |
|-------|-------------|--------------|
| **Domain** | Core entities, repository traits (abstractions), business rules | None (innermost layer) |
| **Use Case** | Business logic orchestration, workflow coordination | Domain (traits) |
| **Infrastructure** | Implements repository traits with AWS services | Domain (traits) |
| **Interface** | Telegram handlers, Lambda handlers | Use Case, Domain |

## Dependency Injection and the Composition Root

The composition root is `src/main.rs`. At runtime it constructs the concrete Infrastructure implementations, wraps them in `Arc`, coerces each into the domain trait object the use cases require, injects them into the use cases, and passes the assembled use cases as a `Deps` struct to the Telegram interface layer.

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

A single `DynamoBotRepository` implements three domain traits — `BotRepository`, `BotRuntimeRepository`, and `StartLockRepository` — so the same `Arc` is coerced into each trait object and shared across the use cases that need it:

```rust
let bots_dyn: Arc<dyn domain::BotRepository> = bot_repository.clone();
let runtimes_dyn: Arc<dyn domain::BotRuntimeRepository> = bot_repository.clone();
let start_locks: Arc<dyn domain::StartLockRepository> = bot_repository.clone();
```

Use cases depend on domain ports rather than concrete infra types — for example the API-key store is injected as `Arc<dyn domain::ApiKeyRepository>`, not the concrete `S3ApiKeyRepository`.

## Binaries

The crate produces two binaries, both built on the same Domain/Use Case/Infrastructure core:

- **`src/main.rs`** — the Telegram bot. It long-polls the Telegram Bot API via teloxide, wires every use case in its composition root (DynamoDB, S3, and ECS clients; `RunTaskUseCase`, `EcsTaskController`, `StartBotUseCase`, `StopBotUseCase`, the bot/template/config use cases), and dispatches updates through the interface layer.
- **`src/bin/task_state_change_handler/`** — an AWS Lambda that listens to ECS **Task State Change** events (RUNNING and STOPPED) delivered via EventBridge.
  - On **RUNNING** it records observed-running state via `RecordRunningTaskUseCase`.
  - On **STOPPED** it parses the stop reason into a `StopInfo` (container `exitCode` + `stopCode`) and delegates the restart-or-skip decision to `ReconcileStoppedTaskUseCase`.

  Together these keep the observed `BotRuntime` state in sync with reality, event by event. The Lambda has its own composition root in `src/bin/task_state_change_handler/main.rs`, performing cold-start initialization once and reusing the same `AppState` across warm invocations. The event parsing lives in `event_handler.rs`: it ignores any event that is not `source = "aws.ecs"` / `detail-type = "ECS Task State Change"`, extracts `USER_ID`/`BOT_ID` from the container override environment (scanning every override, since a name-only sidecar override can sort ahead of the passivbot container), and uses the EventBridge event time as the observation timestamp.

## Telegram Handler Routing

The dispatcher is built in `src/interface/telegram/router.rs`. It installs middleware, then composes three ordered branches; the first matching branch handles the update:

1. **commands** (`commands.rs`) — slash commands such as `/start` and `/list` (a `Command` enum deriving teloxide's `BotCommands`).
2. **callbacks** (`callbacks.rs`) — inline keyboard button presses.
3. **dialogue** (`dialogue.rs`) — stateful multi-step flows (add bot, delete bot, set risk level).

Two in-memory stores back the conversation, injected into the dispatcher via teloxide's `InMemStorage`:

- `DialogueState` — the current flow step.
- `BotContext` — the currently selected bot id.

`keyboards.rs` defines the menu and button layouts.

The dialogue layer renders the **Status** view, which shows both desired and observed state side by side (`dialogue.rs`):

- **Desired** comes from `Bot.enabled` — `🟢 Enabled` / `🔴 Disabled`.
- **Actual** comes from the observed `RuntimePhase` — `⏳ Starting` / `▶️ Running` / `⏹️ Stopped`.

The **Run bot** / **Stop bot** buttons flip desired state **and** actuate ECS by driving `StartBotUseCase` / `StopBotUseCase`.

## Desired State vs Observed State

The model deliberately separates two distinct concepts:

- **Desired state = user intent.** `Bot.enabled` (a `bool`) records whether the user turned the bot on, toggled via `Bot::enable` / `Bot::disable`. There is no `status` attribute on the bot — desired state is `enabled` and nothing else.
- **Observed state = reality.** The `BotRuntime` aggregate (`src/domain/runtime.rs`) records whether the ECS task is actually running. It carries `phase: RuntimePhase`, plus `task_id`, `version` (a restart counter / task generation), and `observed_at`. `RuntimePhase` has three variants:
  - `Running` / `Stopped` — written by the ECS Task State Change Lambda (`RecordRunningTaskUseCase` on RUNNING, `ReconcileStoppedTaskUseCase` on STOPPED).
  - `Starting` — the transient exclusive-start-lock state a launcher stamps the instant it claims the right to launch and before the RUNNING event arrives. It lets a concurrent launch be rejected and lets a stop issued during startup locate the task. The Lambda only ever writes `Running` / `Stopped`.

Observed runtime is read via `GetBotRuntimeUseCase`. `BotRuntimeRepository::find_consistent` provides a strongly-consistent read for decisions that must not act on a stale replica (e.g. stopping a task needs the freshest `task_id`); it defaults to `find` and is overridden by the DynamoDB implementation.

## Auto-restart Reconciliation

`ReconcileStoppedTaskUseCase` (`src/usecase/reconcile_stopped_task.rs`) owns the restart policy. It restarts a stopped task **only** when both conditions hold:

1. **Desired state is ON** — `bot.enabled == true`.
2. **The stop was memory-related** — `StopInfo::is_memory_related()` is true, i.e. `exit_code == 137` and `stop_code` does not contain `UserInitiated`.

The restart is claimed through the **exclusive start lock**, keyed on the stopped task id, so a duplicate or late STOPPED event (EventBridge is at-least-once) cannot spawn a second task. The use case returns one of:

| Outcome | Meaning |
|---------|---------|
| `Restarted { task_id }` | A replacement task was launched. |
| `SkippedNotEnabled` | Desired state is OFF; the user manually stopped it. Recorded as stopped, never restarted. |
| `SkippedNotMemoryRelated` | The stop was not an OOM (e.g. exit 0, or 137 with `UserInitiated`). Recorded as stopped. |
| `SkippedSuperseded` | The stopped task is no longer the row's current task (duplicate/late STOPPED). |
| `BotNotFound` | The bot no longer exists; a stopped runtime is recorded so it is not left showing Running. |

The flow inside `execute` is ordered for safety:

1. Read `prev_version` up front (needed even on the bot-not-found path to record stopped state).
2. If the bot is missing, record stopped and return `BotNotFound`.
3. If `!enabled`, record stopped and return `SkippedNotEnabled` — a bot the user manually disabled is never resurrected, even after an OOM.
4. If the stop is not memory-related, record stopped and return `SkippedNotMemoryRelated`.
5. Claim the restart via `try_acquire_restart`; anything other than `Acquired` returns `SkippedSuperseded`.
6. Re-validate desired state inside the held lock with a strongly-consistent read; if the bot was disabled mid-claim, release the lock and return `SkippedNotEnabled`.
7. Launch the task; on failure release the lock and propagate the error.
8. `attach_started_task` the new task id.
9. Post-launch re-check: if a disable landed during the launch window (after the gate but before the id was attached, so `StopBot` could not see the task), stop the task with the controller so it never trades against an OFF intent, and return `SkippedNotEnabled`.

The lock is stamped with fresh wall-clock `now` (not the possibly-stale EventBridge event time), so a just-claimed restart lock can never look stale to a concurrent telebot start.

## Exclusive Start Lock (no double-run)

A bot must **never** run two live-trading tasks at once. Every launcher — the telebot "Run bot" (`StartBotUseCase`) and the Lambda auto-restart (`ReconcileStoppedTaskUseCase`) — claims an exclusive lock before `RunTask`. The lock is the `StartLockRepository` port (`src/domain/runtime.rs`): a `starting` row guarded by a DynamoDB **conditional write**. The authoritative gate is the atomic write, **not** the read — a strongly-consistent read alone cannot stop two concurrent claimers from both launching.

`StartClaim` is the outcome of a claim attempt: `Acquired` (the caller won and must launch exactly one task), `AlreadyRunning` (a task is already running, nothing to launch), or `AlreadyStarting` (another launch is already in flight). The four lock operations:

- **`try_acquire_start(user_id, bot_id, now, stale_after)`** — the cold-start claim. Atomically transitions the row to `starting`, succeeding only when it is safe to launch: the row is absent/stopped, or holds a `starting` lock older than `stale_after` seconds (an abandoned launch). Concurrent callers are serialized per row, so at most one receives `Acquired`. `StartBotUseCase` uses `START_LOCK_STALE_AFTER_SECS = 600` (deliberately longer than any real task-start latency).
- **`try_acquire_restart(user_id, bot_id, stopped_task_id, now)`** — the Lambda's restart claim. Transitions to `starting` **only** while `stopped_task_id` is still the row's current `task_id`, bumping the restart counter. This is the idempotency gate: duplicate STOPPED events for the same task find the id already cleared and are rejected, so a stopped task is replaced at most once.
- **`attach_started_task(user_id, bot_id, task_id)`** — records the launched `task_id` on the held `starting` lock so a stop issued before the RUNNING event can still find the task. A no-op if the row already advanced past `starting`.
- **`release_start(user_id, bot_id, now)`** — releases a held `starting` lock back to `stopped` after a failed launch. A no-op if the row already advanced past `starting`.

After winning the lock the launcher calls `attach_started_task` on success or `release_start` on launch failure; the Lambda's RUNNING event then flips `starting → running`.

### Stale-lock reclaim and liveness

Before reclaiming a stale `starting` lock that still carries a `task_id`, `StartBotUseCase` confirms via ECS `DescribeTasks` (`TaskController::liveness`, returning `TaskLiveness::Alive` / `TaskLiveness::Gone`) that the task is actually gone. A live task whose RUNNING event was lost is therefore never double-launched: if liveness reports `Alive`, the start returns `AlreadyRunning` without claiming the lock or launching. The residual time-based reclaim applies only when no `task_id` was ever recorded (a crash before it could be attached) — there is no id to verify, so the time window is the accepted edge.

`StartBotUseCase::execute` is ordered: flip desired state ON and save first (so intent survives a launch failure and auto-restart keys off it), then run the liveness guard, then `try_acquire_start`, then launch and `attach_started_task` (or `release_start` on failure). It returns `Started { task_id }`, `AlreadyRunning`, `AlreadyStarting`, or `BotNotFound`.

`StopBotUseCase::execute` flips desired state OFF first — so the STOPPED event from its own `StopTask` (which ECS stamps `UserInitiated`) is reconciled as user-initiated and never auto-restarted — then locates the task by the `task_id` on the runtime row (read strongly-consistently so a just-started task is seen) and issues `StopTask`. It returns `Stopped { task_id }`, `NotRunning`, `StartInProgress` (a launch is mid-flight and its id is not recorded yet; a retry once RUNNING lands will stop it), or `BotNotFound`.

## Layer Details

### Domain Layer (`src/domain/`)

Core business entities and repository interfaces, with no external dependencies:

- `Bot` — trading bot aggregate root with metadata. `Bot.enabled` is the **desired state** (user intent), toggled via `enable` / `disable`.
- `BotRuntime` (`runtime.rs`) — the **observed state** aggregate (`RuntimePhase::{Starting, Running, Stopped}`, `task_id`, `version`, `observed_at`). Kept separate from desired state.
- `BotConfig` — user-specific bot configuration. Owns its business rules: `apply_risk_level` sets the risk and derives leverage (`= max(long, short) + 1`) atomically; `from_template` / `set_live_user` bind the `live.user` field.
- `ConfigTemplate` — reusable configuration templates.
- `Exchange` — supported exchanges (currently Bybit).
- Value objects: `RiskLevel`, `Leverage`, `Coins`. `RiskLevel` / `Leverage` validate on construction (`::new` returns `Result`), so an instance is always in range.
- Errors: `DomainError` (a `thiserror` enum) is the domain failure type.
- Repository ports: `BotRepository`, `BotRuntimeRepository`, `StartLockRepository`, `ApiKeyRepository`, plus the `Clock` port (`SystemClock` in production).

### Use Case Layer (`src/usecase/`)

Business logic orchestration:

- `AddBotUseCase` — create a new trading bot.
- `DeleteBotUseCase` — remove a bot and its associated data.
- `ListBotsUseCase` — retrieve a user's bots.
- `ListTemplatesUseCase` — list available templates.
- `ApplyTemplateUseCase` — apply a template to a bot.
- `GetBotConfigUseCase` — retrieve a bot's configuration.
- `UpdateBotConfigUseCase` — update the full configuration.
- `UpdateRiskLevelUseCase` — adjust risk parameters.
- `StartBotUseCase` — "Run bot": flip desired ON and launch the ECS task behind the exclusive start lock.
- `StopBotUseCase` — "Stop bot": flip desired OFF and stop the running task.
- `GetBotRuntimeUseCase` — read observed runtime (`BotRuntime`) for a bot.
- `RecordRunningTaskUseCase` — record observed-running state on a RUNNING event (returns `Recorded { version }` or `SkippedStale`).
- `ReconcileStoppedTaskUseCase` — decide whether to restart a stopped task; the restart is claimed through the start lock and is idempotent per stopped task.
- `RunTaskUseCase` (`TaskRunner` port) — launch a Passivbot ECS task.
- `EcsTaskController` (`TaskController` port) — stop a task (`StopTask`) and check liveness (`DescribeTasks`).

### Infrastructure Layer (`src/infra/`)

Concrete implementations of the domain ports:

- `DynamoBotRepository` — bot persistence in DynamoDB; also implements `BotRuntimeRepository` (observed-runtime rows) and `StartLockRepository` (the conditional-write start lock).
- `S3BotConfigRepository` — configuration storage in S3.
- `S3TemplateRepository` — template storage in S3.
- `S3ApiKeyRepository` — secure API-key storage.
- `client.rs` — AWS client initialization for DynamoDB, S3, and ECS.

### Interface Layer (`src/interface/telegram/`)

The Telegram bot implementation:

- `router.rs` — teloxide dispatcher setup (middleware + the commands/callbacks/dialogue branches).
- `commands.rs` — slash command handlers (`/start`, `/list`).
- `callbacks.rs` — inline button handlers.
- `dialogue.rs` — conversation state management and the Status view (Desired from `Bot.enabled`, Actual from `RuntimePhase`); the Run/Stop buttons flip desired state and actuate ECS via `StartBotUseCase` / `StopBotUseCase`.
- `keyboards.rs` — menu and button layouts.

The ECS Lambda interface lives separately under `src/bin/task_state_change_handler/`.
