# Terraform Infrastructure Deploy

Infrastructure as code for the dev environment lives under `terraform/`. The
environment root is `terraform/envs/dev`; reusable modules are under
`terraform/modules/`. Run every command below from `terraform/envs/dev`.

The authoritative, step-by-step operations and recovery guide for this
environment is [`../../terraform/envs/dev/RUNBOOK.md`](../../terraform/envs/dev/RUNBOOK.md).
Read it before any `terraform apply` on dev. This page is the orientation; the
runbook is the source of truth for the procedures.

## State backend

State is an **S3 backend with native S3 locking** (no DynamoDB lock table). The
`backend "s3"` block is in `terraform/envs/dev/main.tf`. Backend blocks do not
support variable interpolation, so every value is hardcoded:

| Setting | Value |
|---|---|
| `bucket` | `pbtb-rust-tfstate-025418542265` |
| `key` | `envs/dev/terraform.tfstate` |
| `region` | `ap-northeast-1` |
| `profile` | `dev` |
| `encrypt` | `true` |
| `use_lockfile` | `true` (S3-native lock) |

Account `025418542265`, region `ap-northeast-1`.

The state bucket is created out-of-band and is **not** managed by this
configuration. The bucket and the `dev` AWS profile must already exist before the
first `terraform init`.

## AWS profiles

Two distinct profiles are in play, and both resolve to the same `dev` account:

- The **backend** authenticates with the `dev` profile, hardcoded in the
  `backend "s3"` block (`profile = "dev"`). This is the real account
  (`025418542265`) and must resolve to valid credentials, or `init`/`plan`/`apply`
  cannot read or lock state.
- The **provider** uses `var.profile`. That variable has no default and is
  **required**; `terraform/envs/dev/terraform.tfvars` sets `profile = "dev"`, so
  both the backend and the provider point at the dev account. The `dev` profile
  must exist in your AWS config for either to work.

## Values that stay out of git

`terraform/envs/dev/secrets.auto.tfvars` is gitignored and loaded automatically
by every plan/apply from that directory. It holds `sentry_dsn` (the Sentry
project's write-only key, see `variables.tf`). A fresh checkout without the
file plans an empty DSN, which is valid and simply leaves the binaries logging
only, so a missing file shows up as `APP__SENTRY__DSN` changing to `""` in the
plan rather than as an error: read the plan before applying.

## Basic flow

```bash
cd terraform/envs/dev
terraform init    # configures the S3 backend (no -migrate-state on a fresh checkout)
terraform plan
terraform apply
```

> ⚠️ Do **not** run a bare `terraform apply` (whole-config apply) on dev without
> first reviewing the plan for anything that touches the NAT instance. The NAT is
> both the telebot host **and** the sole egress for all trading traffic, and a
> `user_data`/AMI change forces a destroy + recreate that blackholes egress (see
> below). For routine changes, scope the apply with `-target` to the resources you
> intend to change. Reserve a full apply for a maintenance window where you have
> already reasoned through the plan.

### Prerequisites that the plan evaluates

- **Lambda bootstrap artifact.** `terraform plan/apply/import` here evaluates the
  `task_state_change_handler` Lambda's `archive_file` data source, which needs
  `target/lambda/task_state_change_handler/bootstrap` to exist (built separately).
  Build the lambda first, or the command errors on a missing file.
- **`module.ecr` must be present in config.** The live dev state references
  `module.ecr` for both image repos (`telebot` → `scalable-cluster-dev-telebot`,
  `force_delete=true`; `passivbot_v741` → `passivbot-live`, `force_delete=false`,
  the live trading image). Both repos pre-existed and were adopted into state
  **without recreation** (`telebot` via `state mv`, `passivbot-live` via `import`).
  If the `module.ecr` code is missing from the checkout you apply from, terraform
  sees `module.ecr.*` in state but not in config and tries to **destroy/recreate**
  the repos. **Never** let terraform recreate these repos. Apply only from a
  checkout that contains the `module.ecr` config.

## The NAT instance: egress + telebot host

`module.network.aws_instance.nat` (tag `Name=nat-instance`) carries
`user_data_replace_on_change = true` with **no** `ignore_changes`. Any change to
the NAT bootstrap script (or the AMI) makes `terraform apply` **destroy +
recreate** the NAT.

During the relaunch + cloud-init window:

- **All trading egress is blackholed** — the private route and the EIP re-attach
  to the new instance within the same apply.
- **telebot stays DOWN** until the first `telebot-deploy` writes
  `/etc/telebot/telebot.env` on the new host. The NAT `user_data` carries zero app
  config; the host config is delivered out-of-band.

Because the NAT is the single egress for all trading traffic, treat any
NAT-touching apply as a service interruption.

### What the NAT no longer carries

S3 and DynamoDB gateway endpoints are attached to the private route table, so
those two services are reached without the NAT — bot config I/O, and every ECR
image pull (ECR serves layers from S3 in-region). Gateway endpoints are free and
have no ENI. The exchange traffic and the ECR/SSM/CloudWatch **APIs** still go
through the NAT; those are interface endpoints, billed per hour per AZ.

The endpoints carry no policy, which is deliberate — see
[`RUNBOOK.md`](../../terraform/envs/dev/RUNBOOK.md), "Gateway endpoints".

### Carrying egress across the rebuild: the standby NAT

Two `terraform.tfvars` switches — `nat_standby_enabled` and `nat_egress_active` —
put a throwaway `t4g.nano` NAT in front of the rebuild, so the bots keep trading
through it while the primary is replaced. The default route change is one atomic
`ReplaceRoute`; the whitelisted EIP moves with it. That turns the several-minute
black hole into a few seconds of rejected API calls, twice. telebot is still down
from the rebuild until the next `telebot-deploy` — the standby carries egress,
not the bot.

The switches default to off and the standby costs nothing while it does not
exist. The full step order — including why they must be edited in `tfvars` rather
than passed with `-var` — is in
[`RUNBOOK.md`](../../terraform/envs/dev/RUNBOOK.md), "Rebuilding the NAT without
an egress outage".

### NAT `user_data` maintenance-window procedure (no standby)

This is the fallback for when you accept the outage. Run it whenever a planned
change alters the NAT `user_data` or AMI (for example, the config-decoupling
refactor's first application changes `user_data` and triggers exactly one NAT
rebuild):

1. **Quiesce trading.** Stop the bots; nothing should need egress during the
   window.
2. **Apply, scoped.** Run a `-target`ed apply that rebuilds the NAT and
   republishes the stable telebot config:
   ```bash
   terraform apply \
     -target=module.network.aws_instance.nat \
     -target=aws_ssm_parameter.telebot_base_env
   ```
   This recreates the NAT and (re)publishes `base-env`
   (`/scalable-cluster/dev/telebot/base-env`). If the same change also alters the
   GitHub Actions `gh_deploy` policy / telebot IAM, include those resources in the
   apply scope so the deploy role can re-tag ECR and `SendCommand` to the rebuilt
   NAT host. The `gh_deploy` policy scopes SSM `SendCommand` by the
   `ssm:resourceTag/Name = nat-instance` tag, which survives recreation, so a NAT
   rebuild alone does not require a policy change.
3. **Immediately run `telebot-deploy`.** Trigger the `telebot-deploy` workflow with
   `tag=latest` and `passivbot_revisions=latest`. It composes
   `/etc/telebot/telebot.env` (base-env + the resolved passivbot task-def ARN),
   writes it to the NAT via SSM, and restarts telebot. Until this runs, telebot is
   down.
4. **Verify before resuming.** Confirm egress works again and telebot is running,
   then resume trading.

> Reverting a `user_data`-changing commit re-changes `user_data` → a **second** NAT
> replacement + egress blip. Only do that inside a maintenance window, and rarely.

### When the NAT is NOT involved

If telebot is down but egress is fine, the fix is to re-run `telebot-deploy` — do
**not** revert + apply. Most routine work (shipping a telebot build, rolling back
an image) never touches the NAT lifecycle. The passivbot task-def ARN must stay in
sync between the lambda (baked at apply) and telebot (resolved by `telebot-deploy`):
every passivbot apply is followed by a `telebot-deploy`. See the runbook for the
full operational matrix and the sync rule.
