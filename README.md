# PBTB-Rust

A Telegram bot, written in Rust, for managing [Passivbot](https://github.com/enarjord/passivbot) trading bots on AWS. It gives an interactive Telegram interface for creating, configuring, running, and supervising automated cryptocurrency trading bots.

## Features

- **Bot management** — create, delete, and list trading bots through Telegram
- **Configuration** — apply predefined configuration templates to bots
- **Risk management** — adjust risk levels (long/short exposure); leverage is derived automatically
- **Run / Stop control** — turn a bot on or off; this sets *desired state* (user intent) **and** actuates the ECS task (`RunTask`/`StopTask`) behind an exclusive start lock
- **Auto-restart supervision** — a Lambda reconciles stopped ECS tasks, restarting only when the bot is still enabled and the stop was memory-related (OOM)
- **Secure credentials** — exchange API keys stored encrypted in S3, isolated per user

## Tech Stack

| Area | Choice |
|------|--------|
| Language / runtime | Rust 2024, Tokio |
| Telegram | teloxide 0.12 |
| AWS | DynamoDB, S3, ECS, Lambda (`provided.al2023`), EventBridge |
| IaC / dev | Terraform (S3 backend), Docker / Dev Container |

## Documentation

Detailed docs live under [`docs/`](docs/). Start here:

| Topic | Document |
|-------|----------|
| System design — layers, DI, desired-vs-observed state, the exclusive start lock | [docs/architecture.md](docs/architecture.md) |
| DynamoDB single-table + S3 layout | [docs/data-model.md](docs/data-model.md) |
| Dev Container, build / test, configuration | [docs/development.md](docs/development.md) |
| Code style, comment & git conventions | [docs/conventions.md](docs/conventions.md) |
| **Deployment** — map + safety rules (read first) | [docs/deployment/overview.md](docs/deployment/overview.md) |
| ↳ Terraform infra + the NAT maintenance window | [docs/deployment/infra.md](docs/deployment/infra.md) |
| ↳ Lambda (`task_state_change_handler`) | [docs/deployment/lambda.md](docs/deployment/lambda.md) |
| ↳ telebot (build + deploy) | [docs/deployment/telebot.md](docs/deployment/telebot.md) |
| dev env runbook — NAT / telebot / passivbot ops | [terraform/envs/dev/RUNBOOK.md](terraform/envs/dev/RUNBOOK.md) |

Working in this repo with an AI agent? See [AGENTS.md](AGENTS.md).

## Quickstart

Build and test inside the Dev Container (the host often lacks the native toolchain `aws-lc-sys` needs):

```bash
# VS Code: "Dev Containers: Reopen in Container", then inside app-node:
cargo build
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

Full setup, networking, and configuration: [docs/development.md](docs/development.md).

## Security

`user_id` is the tenant isolation boundary — every row lives under `pk = user_id#<user_id>`, derived from the authenticated Telegram id. API keys are stored encrypted in S3, never logged or surfaced. S3 buckets block all public access; IAM follows least privilege. See [docs/architecture.md](docs/architecture.md) and [docs/data-model.md](docs/data-model.md).
