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

A comment citing lab evidence names the document and section that hold each cited result.

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

Every repository port returns `Result<_, DomainError>` — `BotRepository` (including `delete`), the S3 ports (`ApiKeyRepository`, `BotConfigRepository`, `ConfigTemplateRepository`), and the runtime/lock ports. `DomainError::Repository` carries the de-masked source chain as `context` **and** the underlying error as a `#[source]`, built via `infra::aws_error::repo_err` (which replaced the string-returning `fmt_sdk_err`); a present-but-unparseable row is a `CorruptRecord` fault, never collapsed into `None` or silently dropped from a `Vec`. Use cases propagate the typed error with `?` (`anyhow` only at the Lambda boundary) instead of `e.to_string()` / `{:?}` flattening, and best-effort side-effects log on a dropped `Result` rather than `let _ =`. The Telegram edge redacts every fault through `interface::telegram::redaction`: validation echoes the constraint, everything internal collapses to one opaque line plus a correlation id (full chain logged under that id). Retryability is typed: `DomainError::Repository` carries a `Retryability`, classified in infra (`infra::aws_error::sdk_err`) from the SDK's own error code and transport variant, and read back anywhere via `DomainError::retryability()`. `sdk_err` is deliberately separate from `repo_err`: only a call that reached the network can be classified, and a serialize/parse failure is permanent by construction. Everything unclassifiable is `Permanent` -- including a fault that arrives through an `anyhow` chain, where the SDK's code is already gone -- because that only over-redacts, while the reverse promises a retry that can never work. The Telegram edge renders `Transient` and `Internal` differently in what the user should *do*, while hiding the cause and logging the full chain identically for both.

## Git Workflow

Run `cargo fmt && cargo clippy` before committing.

### Review before a PR

`REVIEW.md` at the repo root is the review policy: three passes (bugs, security, compliance), what is Important and what is a Nit, what to skip, the evidence a finding needs. The `pbtb-ship` skill runs the passes through the reviewer agents before the PR is opened, and the PR's Review section carries the tally; findings never approve or block on their own.

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

## Knowledge placement

Knowledge is filed by *how it reaches an agent's context*, not by topic. Each fact has exactly one home; every other place links to it.

| Layer | Home | Reaches context | Admit when | Leave when |
|-------|------|-----------------|------------|------------|
| Always loaded | `AGENTS.md` (every agent) and `.claude/CLAUDE.md` (imports it; Claude Code harness notes only) | Every session | An irreversible or trading-impacting invariant, or a mistake made twice in this repo | A hook or the code structure enforces it: shrink to one pointer |
| Pulled per task | `docs/*.md` | The "Working on… → Read" table in `AGENTS.md` | Needed for one kind of task; one leaf per task kind, self-contained | The code moved: update in the same PR or delete |
| Triggered by situation | `.claude/skills/<name>/` (`SKILL.md` + `references/`) | The skill's `description` matches | A procedure that must run the same way every time; an operational trap that has already cost time | The skill stops triggering, or drifts from what the code does |
| Isolated | `.claude/agents/`, and `REVIEW.md` at the root (the review policy the reviewer agents and Claude Code Review load) | Explicit dispatch | A review or investigation that would flood the main context | — |
| Zero context | `.claude/hooks/` (`guard-shell.py` refuses an unscoped `terraform apply`, any destroy, `aws ecs run-task`/`stop-task`, and what would reach around the local agent identity, [agents.md](agents.md) § Local agents; `rustfmt-on-edit.sh`), `gate.sh` | Tool calls and the verify gate | Any rule a script can decide | — |
| Records | PR descriptions (decisions), GitHub issues (open items) | Never automatically | Plans, post-mortems, status | Closed when done; no plan files in the tree |

Budgets, checked by the verify gate: `AGENTS.md` ≤ 6 KB, `.claude/CLAUDE.md` ≤ 1.5 KB, `REVIEW.md` ≤ 4 KB, each skill `description` ≤ 300 bytes. A budget is met by demoting a fact to the next layer, never by raising the budget.

- A skill `description` says *when* to use the skill, not what it knows; the body's first paragraph says what the skill knows that the reader does not.
- The always-loaded layer admits a fact on its second occurrence; the other layers on the first, when it cost more than half an hour or touched production.
- A leaf that says when a script skips or refuses a run names every input the check reads, including the ones that switch the check off; a change to the check edits that sentence in the same PR.
- A leaf or docstring that describes a script's data table names the table's role, not a list of the kinds it holds today, unless it lists every kind.
- A PR that changes behaviour updates the leaf, skill, or invariant that described the old behaviour (the PR template asks).
- A change to the unit a page lists or selects (a run, a bot) greps the page's leaf and the comments of the modules the page calls for the old unit's noun, and rewords them in the same PR.
- A row description in `docs/data-model.md` states every value a writer can leave behind, not only the one a reader defaults to; an ops-script writer counts, since it skips the use case's checks.
- An ops write on a row under the tenant partition pins the row kind in its condition (a field only that kind carries), not only the partition: every row of the tenant shares the `pk`.

### Issues

An issue is the interface between human judgment and agent execution: one change's intent, its lifecycle, and the accept-or-reject decision. It is not a place knowledge lives; what is learned while closing it goes to a layer above.

- One template per kind of work, in `.github/ISSUE_TEMPLATE/`: **Intent** (a change: problem, outcome, *done when*, out of scope, invariants touched, how far the agent may go), **Incident** (a symptom, verbatim; the agent starts with `pbtb-triage` and delivers a diagnosis before a patch), **Rollout** (what, window, rollback, done when), **Knowledge drift** (what is written, what is true, which layer gets the fact). Blank issues are off.
- Every template carries a *done when* an agent can run and an autonomy tier (open a PR / merge when green / merge and deploy). Trading actions are never delegated through an issue.
- An agent starts from `gh issue view <n>`, restates *done when* in its PR, and closes with `closes #<n>`. A PR that changes a behaviour an issue's *done when* pins edits that line in the same PR, so the *done when* stays runnable against `main`. Findings an agent makes on its own (a red not in its diff, a `deploy-audit` finding, a stale doc) go in through the same templates via `gh issue create --body-file`, with the label `source:agent` beside the template's own, so a human sees at a glance which issues are their intent and which are an agent's finding awaiting triage. Three label axes, one each: the template's label says which flow the issue enters (`intent`, `incident`, `rollout`, `knowledge`), `bug` / `enhancement` say what kind of change it asks for, `source:agent` says who filed it. A defect a review finds is `intent` + `bug` + `source:agent`: it wants a change with a known cause, not a diagnosis.
- Runtime errors arrive the same way: every binary reports `tracing::error!` events to Sentry, and the `incident-intake` workflow (`scripts/ops/sentry_issues.py`, every six hours) files one Incident issue per unresolved Sentry issue, labelled `incident` + `source:agent`, at the "diagnose only" tier. The Sentry id sits in the body as `<!-- sentry:<id> -->`; that marker is what stops a second filing, so leave it in when editing. Resolving the Sentry issue is a human act, done once the GitHub issue closes.
- The "diagnose only" tier runs itself: the `incident-diagnose` workflow answers every Incident issue a collaborator opens or the intake files with a read-only diagnosis comment (the agent and its bounds: [docs/agents.md](agents.md)). The comment is evidence for the human triage, not a verdict; a higher tier starts from a person, or from the diagnosis itself when the finding is self-evident (below). An Incident closes with a line in the matching section of the triage skill's symptom playbooks saying what to check first next time, as a command; the narrative stays on the issue, and the closing comment links the PR that added the line or says `nothing new`.
- The "open a PR" tier runs on request: a collaborator adds the `agent:fix` label (or dispatches `issue-fix` by number) and the fix agent writes the change on a clean checkout of main and opens `fix/issue-<n>` (the agent, its bounds and whose token publishes: [docs/agents.md](agents.md)). Only collaborators' comments on the issue reach the model. The field *How far the agent may go* is what grants the PR, not the label. A change under `src/` or `tests/` becomes a PR only after a `cargo test` was seen red and then green, any change only after a green gate; the PR carries the gate lines, the red→green evidence, a `REVIEW.md` pass by the same model and `closes #<n>`. At a merge tier the PR is armed to merge itself when `gate` passes (repository settings *Allow auto-merge* and *Automatically delete head branches*, and the ruleset requiring `gate`), unless the patch touches `.claude/**` or `docs/**`, which a person reads first; a deploy stays with a person at every tier.
- Every PR ends with one `Lesson` line in its Knowledge section (the fix agent's and the diagnose agent's answers carry the same line): what would have made the work shorter or safer, with a slug from `REVIEW.md`, or `none`. It is a candidate: the review that meets the slug again (`REVIEW.md`, repeats) is what moves the correction into the layer § Knowledge placement names. A model habit seen twice goes into that agent's `RULES` ([docs/agents.md](agents.md)). An Intent that ticks an invariant box or changes a file agents load has its design reviewed as a comment on the Intent before the branch (`pbtb-ship`, design review).
- Not every issue needs a person to say *start*. A finding is **self-evident** when its body would pass as a task one agent hands another session, the test a session applies before it spawns one: confirmed at `file:line`, not a hunch; self-contained, so a session can act on the body alone and run its *Done when*; bounded, one component, none of the template's invariant boxes ticked, inside the paths the fix agent may write; and too large to fold into the PR that found it. A self-evident finding is filed with `agent:fix` already on and the tier *Merge when the verify gate and CI are green*, so the only human gate is the PR, and that gate is the `gate` check the ruleset on `main` requires: `issue-fix` arms auto-merge on the PR when that check is required, and only then. A session started from a suggested task works at the same tier: review passes, PR, wait for CI, merge. An incident the diagnosis finds self-evident is raised one step, to *open a PR*, since a runtime fault under `src/` deserves a person's eyes before merge. An agent raises the tier only of an issue an agent filed (`source:agent`); a person's choice stands. A merge tier counts only when the issue's author is a member of the repository or the local agent App (matched by its user id, [agents.md](agents.md) § Local agents): anyone can open an issue here and pick a tier, so for an outside author it is held at *open a PR*.

## Do Not

- Do not commit `.env` files or secrets
- Do not skip clippy warnings
- Do not introduce hardcoded credentials

## AI Agent Expectations

- Keep changes minimal and targeted
- Avoid scanning unrelated directories
- Ask before running long or destructive commands
- When changing behavior, add or update tests
