---
name: verify
description: Verification gate for pbtb-rust. Use before committing, pushing, opening or updating a PR, or merging, and on 验证一下 / 跑一下测试 / 能合吗 / clippy 过了吗. Runs fmt on the host and check / clippy -D warnings / tests in the dev container, each with an explicit exit guard.
---

# verify

Run `bash .claude/skills/verify/scripts/gate.sh` and read its final summary.
It decides host vs container automatically, guards every step explicitly, and
prints one line per gate. Pass `--host` to force the host toolchain (fast
signal when Docker Desktop is down) or `--container` to insist on the reference
toolchain. The cargo gates run only when the branch touches something cargo
reads (`src/`, `tests/`, `examples/`, `benches/`, `Cargo.*`, the toolchain and
lint configs, `build.rs`, `.cargo/`, `.devcontainer/`, untracked files
included); on a docs-only branch the line reads
`GATE cargo: skipped` and CI, which always passes `--full`, is the run that
proves it. Pass `--full` to run them anyway.

The container mounts the main checkout at `/app`. From a worktree under
`.claude/worktrees/<name>` the script runs cargo in
`/app/.claude/worktrees/<name>` with `CARGO_TARGET_DIR=/app/target/worktrees/<name>`,
so parallel sessions never overwrite each other's test binaries (the first run
in a worktree is a cold build). A checkout outside the main root is not visible
in the container; the script refuses rather than verify the main tree by mistake.

## What "verified" means here

| Gate | Why it is required |
|---|---|
| `cargo fmt --check` (host) | the format-on-edit hook and CI both assume it; runs on the host because it is a pure formatter |
| `cargo check --workspace --all-targets` | `check` alone skips test targets — half of the last regressions were in tests |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | the repo-wide gate greened in #28; `rust-toolchain.toml` pins host, container and CI to one clippy, so a red is a red everywhere |
| `cargo test --workspace` | with `dynamodb-local` up, `tests/botrepository_test.rs` exercises real condition expressions; in-memory mocks once let a `ValidationException` ship |
| `terraform fmt -check` + `validate` (when `terraform/**` changed) | validate catches interpolation/type errors without credentials; a targeted **read-only plan** is the real proof for state moves and env changes |
| workflow YAML parses (when `.github/workflows/**` changed) | a broken workflow fails only at dispatch time, on main |
| knowledge budgets (`AGENTS.md` ≤ 6 KB, `.claude/CLAUDE.md` ≤ 1.5 KB, `REVIEW.md` ≤ 4 KB, skill `description` ≤ 300 B) | the always-loaded context is paid by every session, and a review policy is applied whole or not at all; docs/conventions.md § Knowledge placement says what to demote instead of growing it |

A runtime change is not verified by tests alone. If the diff touches a launch
path, an env variable, or IAM, the verification includes the matching probe
after deploy: `python scripts/ops/pbtb_ops.py smoke-lambda <fn>`,
`bot-status`, `deploy-audit`. Say explicitly which of these you ran.

## Rules

- Never report "verified" from a chain you did not read the exit codes of.
  The gate script prints `GATE <name>: ok|FAIL`; quote those lines.
- Host-green is an early signal, not the result, when the change touches
  clippy-sensitive code; re-run in the container before merging.
- If Docker is down, say so, run `--host`, and leave the container gate queued
  (`until docker exec app-node true; do sleep 5; done` then the script) rather
  than skipping it silently.
- Do not "fix" a red gate by widening `#[allow]`s unless the PR that introduced
  the pattern already established that convention (e.g. inherent `from_str`
  returning `Option`).
