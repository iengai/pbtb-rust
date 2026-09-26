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

`REVIEW.md` at the repo root is the review policy: three passes (bugs, security, compliance), what is Important and what is a Nit, what to skip, the evidence a finding needs. The `pbtb-ship` skill runs the passes through the reviewer agents before the PR is opened, and the PR's Review section carries the tally. An open Important keeps a local agent PR from merging itself: the `Review-verdict` line the `contract` check reads passes only at 0 important ([governance.md](governance.md) § What GitHub enforces).

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
- A fact enters a layer only after the counterfactual: without this text, would the next session take the same detour? When a hook or the gate refuses the action, a test goes red, or the error text or the code itself says so, the answer is no, and the fact goes nowhere but the PR's `Lesson` line. Past that test, the always-loaded layer admits a fact on its second occurrence; the other layers on the first, when it cost more than half an hour or touched production.
- A review finding that is not a REPEAT is fixed in the file it cites and adds no line to a docs leaf, a skill or an agent definition unless the first-occurrence clause above holds, and none to an agent's `RULES` (docs/agents.md § Rules the model gets); its `Lesson` line is the record, and the second occurrence promotes.
- Pruning is demotion, verified by the repeat check. The gate prints a `headroom` line for a budgeted file within a tenth of its ceiling; that is the prompt to demote. The PR that demotes a fact lists it in its Knowledge section as `- [cat:<slug>] demoted <file:line> — <rule>`, so a later finding on that slug is a REPEAT and the correction returns with its evidence. A fact with no slug is deleted once the counterfactual says a hook, the gate or a test covers it.
- A leaf that says when a script skips or refuses a run names every input the check reads, including the ones that switch the check off; a change to the check edits that sentence in the same PR.
- A change to a module constant, or to which one a code path reads, edits every script docstring and argparse help that names the constant in the same commit.
- A change to what a stored or published field means greps for the phrase stating the old meaning and rewords every docstring, comment and user-facing label that carries it in the same PR.
- A leaf row that says where a property comes from covers every class of template that carries the property, the ones that predate the rule included.
- A leaf or docstring that describes a script's data table names the table's role, not a list of the kinds it holds today, unless it lists every kind.
- A PR that adds a deployable component (a Lambda, a workflow target, an incident-template option) adds its row to the `pbtb-triage` component map (`.claude/skills/pbtb-triage/references/component-map.md`) in the same PR.
- A PR that changes behaviour updates the leaf, skill, or invariant that described the old behaviour (the PR template asks). One that removes a user-facing surface (a button, route, tool or dialogue) or replaces what a card's tag or a list's filter shows greps the root `README.md` and every `docs/` leaf for the surface's name and rewords or removes each mention.
- A dependency version bump greps the root `README.md` and every `docs/` leaf for the crate's name and old version and edits each mention in the same PR, and rereads the `Cargo.toml` comment above the dependency for the users and features it names.
- A change that adds or removes a caller of a repository method, or changes one of its conditions, rereads that method's comments for the callers and rules they name, and rewords them in the same PR.
- A documented cost of a rule that refuses a write names the row state each listed case leaves, and what a writer landing after the refused write turns it into.
- A change to the unit a page lists or selects (a run, a bot), to where the page reads it from (a path, a host), or to how soon it follows a write, greps every `docs/` leaf, the page's leaf and the comments of the modules the page calls for the old noun, path or delay, and rewords them in the same PR.
- A change to the field set of a published JSON under `site/templates/` or `site/data/`, or of a block a docs leaf enumerates (a template's `pbtb`, `lab`), greps `site/README.md` and every `docs/` leaf for that field list, and edits it in the same PR.
- A change to *how* a committed artifact's derived block is computed bumps that block's staleness key in the same commit (`backtest_templates.FILL_SHARES_REV` in `capital_profile`'s `key`). The key says what the block was derived from, so a key that names only the inputs keeps a block the current code would not produce, and the next ordinary run writes it beside freshly derived fields in one file.
- A row description in `docs/data-model.md` states every value a writer can leave behind, not only the one a reader defaults to; an ops-script writer counts, since it skips the use case's checks.
- An ops write on a row under the tenant partition pins the row kind in its condition (a field only that kind carries), not only the partition: every row of the tenant shares the `pk`.
- A PR that adds or changes a template names its audience in the store's two words, published or retired ([config-transfer.md](config-transfer.md) § `audience`); "unpublished" is not a state the store holds, and a template with no `audience` is already offered to every member.
- A data-only commit under `site/templates/` states per-field counts taken from a structural diff of the artifacts (how many changed `audience`, `title`, `source_sha`), not the file count, gives each change its actual cause, and names every S3 object the run wrote (the audience overlay `annotate_templates.py --apply` republishes counts).
- A commit or PR body that quotes a figure from `site/templates` reads it off the artifacts over every template of the class it names, not off the examples that prompted the change.
- What makes a template script skip or refuse a template has one home, that script's docstring (`backtest_templates.py`, `describe_templates.py`); `site/README.md` and `docs/config-transfer.md` link to it and do not restate the list, so a refusal added to the script cannot leave a leaf naming only the older ones.
- A commit body that names a failure mode (an exception, a refusal) states the one read off the code path, with the call site; "raised KeyError" for a lookup the caller catches is a guess.
- A PR that lists, withholds or archives a template on a criterion [config-transfer.md](config-transfer.md) § Archiving one does not state edits that leaf in the same PR.

### Issues and autonomy

The issue templates, the autonomy tiers and what GitHub enforces before an agent's PR merges itself: [governance.md](governance.md).

## Do Not

- Do not commit `.env` files or secrets
- Do not skip clippy warnings
- Do not introduce hardcoded credentials

## AI Agent Expectations

- Keep changes minimal and targeted
- Avoid scanning unrelated directories
- Ask before running long or destructive commands
- When changing behavior, add or update tests
