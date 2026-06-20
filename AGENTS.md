# AGENTS.md

Guidance for AI agents (Claude Code, Codex, etc.) working in this repo. This file is the agent entry point: it carries the **load-bearing invariants inline** (below) and an index into [`docs/`](docs/) for everything else. Read the invariants before changing code or infra.

## Quick Context

- PBTB-Rust is a Telegram bot for managing Passivbot trading bot configurations on AWS.
- Architecture: **Interface → Use Case → Domain ← Infrastructure** (Clean Architecture / DDD). Composition root: `src/main.rs`.
- Two binaries: the telebot (`src/main.rs`, teloxide long-poll) and the `task_state_change_handler` Lambda (`src/bin/task_state_change_handler/`, ECS task-state events).

## Documentation map

Detailed docs are flat leaves under `docs/` — open the one for your task directly (README indexes the same set for humans):

| Working on… | Read |
|-------------|------|
| Domain/usecase/infra/interface design, the start lock, desired-vs-observed state | [docs/architecture.md](docs/architecture.md) |
| DynamoDB / S3 schema | [docs/data-model.md](docs/data-model.md) |
| Building, testing, running locally, configuration | [docs/development.md](docs/development.md) |
| Code style, comments, git/branch/commit rules | [docs/conventions.md](docs/conventions.md) |
| **Deploying anything** (start here) | [docs/deployment/overview.md](docs/deployment/overview.md) |
| Terraform infra / NAT | [docs/deployment/infra.md](docs/deployment/infra.md) · [terraform/envs/dev/RUNBOOK.md](terraform/envs/dev/RUNBOOK.md) |
| Lambda deploy | [docs/deployment/lambda.md](docs/deployment/lambda.md) |
| telebot deploy | [docs/deployment/telebot.md](docs/deployment/telebot.md) |

## Critical invariants (do not violate)

These are irreversible or trading-impacting; they are inline here on purpose, not behind a link.

- 🔴 **Terraform / NAT egress.** In `terraform/envs/dev`, never run a blanket `terraform apply`. The NAT instance is the **sole egress for all trading traffic** and the telebot host, with `user_data_replace_on_change = true` — any `user_data`/AMI change **destroys + recreates** it, blackholing trading egress and taking telebot down until the next telebot-deploy. Treat such changes as a maintenance-window op **with trading quiesced first**; scope every other apply with `-target`. Any plan/apply needs `target/lambda/task_state_change_handler/bootstrap` to exist (the lambda `archive_file`). The two ECR repos are already adopted into state — never let Terraform recreate them. → [docs/deployment/overview.md](docs/deployment/overview.md)
- 🔴 **No double live-trading task.** A bot must never run two live tasks at once. Every launcher — the telebot "Run bot" and the Lambda auto-restart — claims the **exclusive DynamoDB start lock** (a conditional write, the gate) before `RunTask`. Do not add a launch path that bypasses it. → [docs/architecture.md](docs/architecture.md)
- 🔴 **Tenant isolation + secrets.** Every row lives under `pk = user_id#<user_id>`. Derive `user_id` from the authenticated Telegram id, never from client input; treat any cross-`user_id` access as a privilege-escalation bug. Never log or surface `api_key` / `secret_key`.
- 🔴 **Comments describe code as-is.** No edit/process narration ("previously/now/no longer", "counterpart to X"). Keep comments for the non-obvious *why*. The `comment-reviewer` agent enforces this on diffs. → [docs/conventions.md](docs/conventions.md)
- **Build/test only inside the Dev Container** (`aws-lc-sys` needs NASM/cmake; the host often can't build). → [docs/development.md](docs/development.md)

## Repo layout (key paths)

- `src/domain/` — core entities + repository traits (no external deps)
- `src/usecase/` — business-logic orchestrations
- `src/infra/` — AWS implementations (DynamoDB, S3, ECS)
- `src/interface/telegram/` — Telegram handlers (router: commands → callbacks → dialogue)
- `src/bin/task_state_change_handler/` — the ECS task-state Lambda
- `config/` — layered config (see [docs/development.md](docs/development.md))
- `terraform/` — AWS IaC; deploy via `terraform/envs/dev/`
- `.devcontainer/` — Dev Container + the `lambda-export` build stage

## Working agreements

- Run `cargo fmt && cargo clippy` before committing.
- Branch `<type>/<kebab-summary>`; commit `<type>: <summary>` (lowercase imperative, ≤72 chars). Types: feat/fix/refactor/test/chore/docs. Details in [docs/conventions.md](docs/conventions.md).
- Keep changes minimal and targeted; ask before long or destructive commands; update or add tests when behavior changes.
- Do not commit secrets or `.env` files; do not introduce hardcoded credentials.
