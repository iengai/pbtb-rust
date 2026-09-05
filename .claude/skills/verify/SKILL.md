---
name: verify
description: Project verification gate for pbtb-rust. Use it before committing, pushing, opening or updating a PR, or merging anything — and whenever the user asks "验证一下 / 跑一下测试 / 能合吗 / clippy 过了吗". It runs the exact gates this repo relies on (fmt on the host, check / clippy -D warnings / tests in the dev container with dynamodb-local, terraform fmt+validate when infra changed) with explicit exit guards, because `set -e` does not stop multi-step commands in this harness and a green-looking chain has merged red code here before.
---

# verify

Run `bash .claude/skills/verify/scripts/gate.sh` and read its final summary.
It decides host vs container automatically, guards every step explicitly, and
prints one line per gate. Pass `--host` to force the host toolchain (fast
signal when Docker Desktop is down) or `--container` to insist on the reference
toolchain.

## What "verified" means here

| Gate | Why it is required |
|---|---|
| `cargo fmt --check` (host) | the format-on-edit hook and CI both assume it; runs on the host because it is a pure formatter |
| `cargo check --workspace --all-targets` | `check` alone skips test targets — half of the last regressions were in tests |
| `cargo clippy --workspace --all-targets --all-features -- -D warnings` | the repo-wide gate greened in #28; the dev container's clippy 1.88 is the reference — a newer host clippy may flag more, an older one less |
| `cargo test --workspace` | with `dynamodb-local` up, `tests/botrepository_test.rs` exercises real condition expressions; in-memory mocks once let a `ValidationException` ship |
| `terraform fmt -check` + `validate` (when `terraform/**` changed) | validate catches interpolation/type errors without credentials; a targeted **read-only plan** is the real proof for state moves and env changes |
| workflow YAML parses (when `.github/workflows/**` changed) | a broken workflow fails only at dispatch time, on main |

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
