# Deployment Overview

This system deploys through three independent surfaces. Each ships a different
piece, on a different mechanism, with a different blast radius. Know which one
you are touching before you touch it.

| Surface | What it ships | Mechanism | Touches the NAT? |
| --- | --- | --- | --- |
| Infra | All AWS resources (network, NAT, ECS, DynamoDB, S3, ECR, the Lambda's config/wiring) | `terraform` in `terraform/envs/dev` | Yes — a `user_data`/AMI change replaces it |
| Lambda code | `task_state_change_handler` bootstrap binary | `lambda-deploy` GitHub Actions workflow (`aws lambda update-function-code`, bypasses Terraform) | No |
| telebot | telebot container image + its on-host config | `telebot-build` (push to `main`) then `telebot-deploy` (manual) | No |

Details per surface: [infra](./infra.md), [lambda](./lambda.md),
[telebot](./telebot.md). Environment-specific operational procedures:
[dev RUNBOOK](../../terraform/envs/dev/RUNBOOK.md).

## Golden safety rules

These are load-bearing. Read them before any deploy.

### 1. Never casually `terraform apply` the whole env

The NAT instance (`module.network.aws_instance.nat`, tag `Name=nat-instance`) is
the **sole egress for all trading traffic** *and* the **telebot host**. It carries
`user_data_replace_on_change = true` with no `ignore_changes`, so **any** change to
its bootstrap script or AMI makes `terraform apply` **destroy and recreate** it.

During the relaunch + cloud-init window:

- All trading egress is blackholed (the private route and EIP re-attach to the new
  instance in the same apply).
- telebot stays **down** until the first `telebot-deploy` writes
  `/etc/telebot/telebot.env`.

A `user_data`/AMI apply is a **maintenance-window operation**: quiesce trading
first (stop bots; nothing should need egress), apply, then immediately run
`telebot-deploy` to bring telebot back up, and verify egress before resuming.

For everything else, **scope unrelated changes with `-target`** so an apply never
sweeps the NAT in by accident.

### 2. Build the Lambda bootstrap before any plan/apply in this env

The Lambda base module's `data.archive_file` (`terraform/modules/lambda/base/main.tf`)
zips `target/lambda/task_state_change_handler/bootstrap`. That data source is
evaluated on **every** `terraform plan`, `apply`, or `import` in `envs/dev`. If the
bootstrap file does not exist, the command errors on the missing file. Build the
Lambda first.

### 3. Never let Terraform recreate the ECR repos

Both image repos are managed by `module.ecr` and were adopted into live state
**without recreation**:

- `telebot` → `scalable-cluster-dev-telebot` (scan-on-push, `force_delete=true`).
- `passivbot_v741` → `passivbot-live` (`scan_on_push=false`, `force_delete=false` —
  matches the live repo so the live trading image is never auto-deleted).

The live state references `module.ecr.*`, so the `module.ecr` code must be present
in **any** checkout used for `terraform apply`. If it is missing from config while
present in state, Terraform will try to **destroy and recreate** the repos —
destroying the live passivbot trading image. Apply only from a checkout that has
`module.ecr` (e.g. merge to `main` first). Never let Terraform recreate these repos.

## How each surface works

### Infra — Terraform (`terraform/envs/dev`)

State lives in the S3 backend (`bucket = pbtb-rust-tfstate-025418542265`,
`key = envs/dev/terraform.tfstate`, `region = ap-northeast-1`) with the S3-native
lock (`use_lockfile = true`, no DynamoDB lock table). Both the backend and the
provider use the `dev` AWS profile.

Terraform owns infrastructure and stable config: it publishes telebot's stable
config to the `base-env` SSM parameter (`/scalable-cluster/dev/telebot/base-env`)
on every apply, registers passivbot task-def revisions, and owns the Lambda's
config/wiring (but not its code — see below).

### Lambda code — `lambda-deploy` workflow

`task_state_change_handler` ships code out-of-band via the `lambda-deploy` GitHub
Actions workflow (manual `workflow_dispatch`), **not** Terraform. The workflow
builds the bootstrap through the devcontainer's `lambda-export` Docker stage
(`rust:1.89-bullseye`, glibc 2.31 < AL2023 2.34) and ships it with
`aws lambda update-function-code`. It never touches the env S3 state, the backend
lock, or the NAT.

Because of this, `aws_lambda_function.this` carries
`lifecycle { ignore_changes = [source_code_hash] }`: a CI-shipped binary is **not**
treated as drift and reverted on the next apply. To deploy Lambda code through
Terraform in an emergency, `-replace` the function.

### telebot — `telebot-build` + `telebot-deploy`

Two workflows:

- **`telebot-build`** (push to `main`, paths-filtered to source/Dockerfile changes;
  also `workflow_dispatch`): builds the `linux/arm64` image and pushes it to the
  `telebot` ECR repo, tagged with a source-content hash, the git SHA, and `latest`.
  A content-hash gate skips the build when an image for that exact source already
  exists. It does **not** touch the running NAT.
- **`telebot-deploy`** (manual `workflow_dispatch`): resolves the passivbot task-def
  ARN, composes `/etc/telebot/telebot.env` (Terraform's `base-env` plus that ARN),
  writes it to the NAT over SSM, and restarts the telebot service. All app config is
  injected here at deploy time; the NAT's `user_data` carries none of it. No build,
  no Terraform, no NAT replacement.

The Telegram token is never in `base-env`; `run-telebot.sh` fetches it live from
SSM (`/scalable-cluster/dev/telebot/teloxide-token`) at container start.

## passivbot revision: keep Lambda and telebot in sync

Two consumers resolve the passivbot task-def from **different** sources:

- **Lambda** (auto-restart on OOM): the revisioned ARN baked at `terraform apply`.
- **telebot** (user "Run bot"): the ARN resolved by `telebot-deploy` at deploy time.

**Rule: every passivbot apply is followed by a `telebot-deploy`** (default
`passivbot_revision=latest` matches the just-applied revision). Skip it and the
Lambda restarts at the new revision while telebot still launches the old one. A
deliberate telebot-only rollback (`passivbot_revision=<n>`) knowingly diverges from
the Lambda until the next apply. See the [dev RUNBOOK](../../terraform/envs/dev/RUNBOOK.md)
for the full procedure.

## Recovery quick reference

- **telebot down, egress fine:** re-run `telebot-deploy` (no NAT impact). This is
  the default fix — do **not** revert and apply.
- **Reverting a `user_data` change:** re-changes `user_data` → a second NAT
  replacement + egress blip. Maintenance window only.
