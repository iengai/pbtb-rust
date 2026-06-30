# Code & Contribution Conventions

## Code Style & Conventions

- Rust 2024 edition
- Prefer `anyhow::Result` in application code, `thiserror` for domain errors
- Use `async-trait` for async trait definitions
- Avoid `panic!`, `unwrap()`, `expect()`; use `?` + context
- Keep domain layer free of external dependencies
- Domain fallibility uses the `DomainError` enum (`thiserror`), not `Result<_, String>`. How errors cross layers and reach the user is specified in [Error Handling](#error-handling)
- Value objects validate on construction: `RiskLevel::new`/`Leverage::new` return `Result`, so any instance is guaranteed in-range
- Keep business rules inside the entity. `BotConfig` owns its invariants: `apply_risk_level` sets the risk and derives leverage (`= max(long, short) + 1`) atomically; `set_live_user` binds `live.user`; `from_template` is fallible and binds `live.user` on construction. Do not re-implement the leverage-derivation rule in the use-case layer.

### Comments

Comments describe the code as it is, for a reader who never saw the diff. Do not narrate the change or the act of writing it: no "previously/now/no longer", "not just the first", "this replaces…", and do not frame new code by its pairing ("the counterpart to X", "together they…"). That is commit-message material. Keep comments for the non-obvious *why* — invariants, gotchas, ordering rules, external constraints — and cut anything that merely restates the code or only parses if you watched it being written. The `comment-reviewer` agent enforces this on the diff.

## Error Handling

One line: **classify errors by what the *reader* must do, not by which layer produced them.** Three readers, three duties — propagate, record, redact.

### Two error classes

- **Business errors** — domain construct/validation failures (`DomainError::{RiskOutOfRange, LeverageOutOfRange, MissingConfigPath, InvalidConfig}`) and the expected branches of a use case. Expected use-case branches are **outcome enums** (`StartClaim`, `StartOutcome`, `ReconcileOutcome`, …), **not** `Err` — reserve `Err` for genuine faults. Business errors are the user's own domain: safe and useful to expose with specifics.
- **Technical / infra faults** — throttle, timeout, network, permission, serialization. They belong to infra and are opaque to the user. They cross the port boundary as `DomainError::Repository`, carrying the underlying error (e.g. the `SdkError`) as a `#[source]` so the chain survives.

### Don't mirror the layers with error types

- Repository traits are **domain-owned**, so a port's error type is part of the **domain contract**: ports return `Result<_, DomainError>`. Never put an infra-defined error type in a port signature — that inverts the dependency rule. Infra may use its own error type internally, but maps it into `DomainError::Repository` at the trait boundary.
- There is **no `UsecaseError`**. Use cases express expected branches as outcome enums and propagate genuine faults via `?` (`anyhow` at the app boundary, or the `DomainError` itself). A per-layer error tower buys ceremony, not safety — nothing branches on error *origin*.

### Absence is not failure

- Read ports return `Result<Option<T>, DomainError>`: `Ok(None)` = the row genuinely does not exist; `Err` = the read failed. **Never collapse a fault into `None` / empty `Vec` / `.ok()?`.** A swallowed read error that reads back as "not found" has silently abandoned a live bot's OOM restart — this is the rule that bug taught.

### Three readers, three duties

- **Propagate** (to the caller / policy owner): the *occurrence* of a fault always surfaces, as an opaque signal carrying its `source`. Infra owns the error's *taxonomy* and *mechanical retry* (the SDK already retries transients); the **caller owns the consequence** — only it knows whether this read was a money-critical reconcile (fail → let EventBridge redeliver) or a best-effort status fetch (degrade). So the fault must reach the layer that holds the policy.
- **Record** (to operators): mandatory on every error path. Log the full chain with `tracing` (`{e:#}` for `anyhow`) and `user_id` / `bot_id` / `task_id` fields. Best-effort side-effects that drop a `Result` use `if let Err(e) = … { tracing::warn!(…) }` — never a silent `let _ =`.
- **Redact** (to the user): at the interface edge, hide the *cause*, keep the *consequence*. Map to a small closed category plus a correlation id; the full detail lives only in logs.

### The user-facing contract (categories)

A small, stable, closed set — like HTTP status classes — keyed on **what the user does**, not why it failed. Exposure is inversely proportional to how internal the error is: validation is shown with specifics; everything internal collapses to one opaque category plus a ref. The contract is transport-agnostic — here the edge is Telegram, not HTTP, so a category renders as message + keyboard + ref, not a status code.

| Category | Telegram rendering |
|----------|--------------------|
| `Validation` | echo the constraint — "risk must be in [0, 10]" |
| `NotFound` | "bot not found" |
| `Conflict` | the business-outcome copy — "already running / stopping" |
| `Transient` | "temporarily unavailable, please retry" (+ a retry affordance) |
| `Internal` | "something went wrong, it's been logged (ref: …)" — no detail, no retry |

### Retryability is the axis that matters

Whether a fault is **transient** (throttle/timeout/network → retry / redeliver) or **permanent** (permission/validation → fail-fast, alarm, DLQ) cuts across all layers and drives the real decisions. Permission errors are the one infra fault that must surface *loudest* — retrying never fixes a missing IAM grant. If you add typing to an error, add it on this axis, not on package structure.

### Secrets

`api_key` / `secret_key` never appear in an error, a log line, or a user-facing message (see the tenant-isolation invariant in `AGENTS.md`). Keep them out of any `Debug`/`Display` that can reach a sink.

### Current state

Every repository port returns `Result<_, DomainError>` — `BotRepository` (including `delete`), the S3 ports (`ApiKeyRepository`, `BotConfigRepository`, `ConfigTemplateRepository`), and the runtime/lock ports. `DomainError::Repository` carries the de-masked source chain as `context` **and** the underlying error as a `#[source]`, built via `infra::aws_error::repo_err` (which replaced the string-returning `fmt_sdk_err`); a present-but-unparseable row is a `CorruptRecord` fault, never collapsed into `None` or silently dropped from a `Vec`. Use cases propagate the typed error with `?` (`anyhow` only at the Lambda boundary) instead of `e.to_string()` / `{:?}` flattening, and best-effort side-effects log on a dropped `Result` rather than `let _ =`. The Telegram edge redacts every fault through `interface::telegram::redaction`: validation echoes the constraint, everything internal collapses to one opaque line plus a correlation id (full chain logged under that id). The one axis left to add when a concrete need arises is typed retryability on `Repository` (transient vs permanent), which would let the edge separate `Transient` from `Internal` instead of folding both into `Internal`.

## Git Workflow

Run `cargo fmt && cargo clippy` before committing.

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
