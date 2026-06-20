# Code & Contribution Conventions

## Code Style & Conventions

- Rust 2024 edition
- Prefer `anyhow::Result` in application code, `thiserror` for domain errors
- Use `async-trait` for async trait definitions
- Avoid `panic!`, `unwrap()`, `expect()`; use `?` + context
- Keep domain layer free of external dependencies
- Domain fallibility uses the `DomainError` enum (`thiserror`), not `Result<_, String>`; the use-case layer still surfaces `String` to the interface
- Value objects validate on construction: `RiskLevel::new`/`Leverage::new` return `Result`, so any instance is guaranteed in-range
- Keep business rules inside the entity. `BotConfig` owns its invariants: `apply_risk_level` sets the risk and derives leverage (`= max(long, short) + 1`) atomically; `set_live_user` binds `live.user`; `from_template` is fallible and binds `live.user` on construction. Do not re-implement the leverage-derivation rule in the use-case layer.

### Comments

Comments describe the code as it is, for a reader who never saw the diff. Do not narrate the change or the act of writing it: no "previously/now/no longer", "not just the first", "this replaces…", and do not frame new code by its pairing ("the counterpart to X", "together they…"). That is commit-message material. Keep comments for the non-obvious *why* — invariants, gotchas, ordering rules, external constraints — and cut anything that merely restates the code or only parses if you watched it being written. The `comment-reviewer` agent enforces this on the diff.

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
