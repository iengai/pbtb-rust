# dev runbook — NAT / telebot / passivbot operations

The NAT instance (`module.network.aws_instance.nat`, tag `Name=nat-instance`) is
**both** the telebot host **and** the sole egress for all trading traffic. Read
this before `terraform apply` on this env.

## Config model (who injects what)

- **NAT `user_data`** = NAT/host bootstrap only. **Zero app config.** It installs
  docker + `run-telebot.sh` + the `telebot.service` unit.
- **`run-telebot.sh`** reads all config from `/etc/telebot/telebot.env` and
  fetches the Telegram token from SSM (`/scalable-cluster/dev/telebot/teloxide-token`)
  at container start. Until that file exists it exits 1 and systemd retries (30s).
- **terraform** publishes the stable config to the `base-env` SSM String param
  (`/scalable-cluster/dev/telebot/base-env`) on every apply.
- **`telebot-deploy`** (GitHub Actions, manual) resolves the passivbot task-def
  ARN, composes `/etc/telebot/telebot.env` (base-env + that ARN), writes it to the
  NAT via SSM, and restarts telebot.

So: **terraform owns infra/stable config; telebot-deploy owns the app config that
lands on the host.** App/image/passivbot churn never touches the NAT lifecycle.

## ⚠️ Applying a change that alters `user_data` → the NAT is REPLACED

`aws_instance.nat` has `user_data_replace_on_change = true` and **no**
`ignore_changes`. Any change to the bootstrap script (or the AMI) makes
`terraform apply` **destroy + recreate** the NAT. During the relaunch + cloud-init
window **all trading egress is blackholed** (the private route + EIP re-attach to
the new instance in the same apply), and **telebot stays DOWN until the first
`telebot-deploy` writes `/etc/telebot/telebot.env`.**

Procedure (maintenance window):
1. Pause/*quiesce* trading (stop bots; nothing should need egress).
2. `terraform apply` (scoped if possible, e.g. `-target=module.network.aws_instance.nat`
   plus the `aws_ssm_parameter.telebot_base_env`). This rebuilds the NAT and
   (re)publishes base-env.
3. **Immediately** run the **telebot-deploy** workflow (tag `latest`,
   `passivbot_revisions=latest`). This writes the env file and brings telebot up.
4. Verify telebot is running and egress works before resuming trading.

> The **first** application of the config-decoupling refactor is exactly this:
> it changes `user_data`, so it triggers one NAT rebuild. Treat it as the above.

## Normal operations (no NAT impact)

- **Ship a new telebot build:** push to `main` → `telebot-build` builds+pushes →
  run **telebot-deploy** (`tag=latest`). Re-pulls the image + rewrites env + restart.
- **Roll telebot back to an older image:** telebot-deploy with `tag=<git-sha>`.
- **Bump a passivbot line:** edit that line's `image_tag` in `var.passivbot_engines` → scoped `terraform apply`
  (registers a new task-def revision; the lambda picks it up at apply) → **then run
  telebot-deploy** (`passivbot_revisions=latest`) so telebot also launches the new
  revision. See the divergence rule below.

## passivbot engine lines: which image a bot launches on

A bot never launches on "the passivbot image". It launches on the task
definition registered for the **engine line its config targets**: the major of
the config's `config_version` (`v7.12.0` -> 7, `v8.1.0` -> 8). A legacy config
with no stamp is classified by shape (only the v8 schema nests
`bot.<side>.risk`); a stamp that is present but unparseable is refused, never
guessed. Rationale: a strategy is only proven on the engine it was validated on
-- v8 broke the v7 schema, and a migrated config is not the same strategy.

Both launchers route this way, so they can never disagree:
- telebot **Run** (`StartBotUseCase`) and the **Choose config** confirmation
  (which refuses a config whose line has no registered image),
- the lambda **auto-restart** (`ReconcileStoppedTaskUseCase`), which reads the
  bot's current config from the config bucket.

Source of truth: `var.passivbot_engines` in `terraform.tfvars` -> one task-def
family per line (`…-passivbot` for the inherited "7" line, `…-passivbot-v8`, ...)
-> `local.td_passivbot_by_engine` = `7=<arn>,8=<arn>`.

Two consumers still resolve that table from **different** sources -- same
sync rule as before, now per line:
- **lambda**: `APP__ECS__TD_PASSIVBOT_BY_ENGINE`, revisioned ARNs baked at
  `terraform apply`.
- **telebot**: **telebot-deploy** reads the families from base-env
  (`PBTB_PASSIVBOT_FAMILIES`) and resolves each to its current revision
  (`passivbot_revisions=latest`), or pins a line (`7=12,8=latest`).

**Rule: every passivbot apply is followed by a telebot-deploy.** A deliberate
telebot-only pin knowingly diverges from the lambda until the next apply.

### First apply of the per-engine split

The migration from the single `module.passivbot_task` to the per-line map is
carried by chained `moved` blocks. Terraform refuses a targeted plan that leaves
a moved instance out, so this one apply must target the whole map, not a single
line:

`terraform apply -target=module.passivbot_task -target=module.lambda_task_state_change_handler -target=aws_ssm_parameter.telebot_base_env`

Expected: 3 to add (the "8" family + log group, the lambda's config-read
policy), 3 to change (the "7" task definition -- only its `Version` tag, family
and revision untouched; the lambda env; the base-env parameter), 0 to destroy.
Then `telebot-deploy` (`passivbot_revisions=latest`).

Both binaries change shape with this apply: the lambda now reads
`APP__ECS__TD_PASSIVBOT_BY_ENGINE` (+ `APP__S3__*`), telebot reads the same
table. An old binary with the new env -- or the new binary with the old env --
fails at config load. So do it back-to-back: merge (telebot-build pushes the new
`:latest`) -> this apply -> `telebot-deploy` -> `lambda-deploy`. Until
lambda-deploy lands, an OOM in that window is not auto-restarted (telebot Run
still works once telebot-deploy is done).

### Adding an engine line (e.g. v9)

1. Build + push the image: `python scripts/build_passivbot_image.py --tag v9.0.0-arm64`.
2. Add `"9" = { image_tag = "v9.0.0-arm64", memory = <measured MiB> }` to
   `passivbot_engines`. Do NOT set `family_suffix = ""` -- that is reserved for
   the one line that inherited the original family.
3. Scoped apply -- the new family, the lambda (bakes the table), and the telebot
   base-env parameter in `telebot.tf` (carries the families):
   `terraform apply -target='module.passivbot_task["9"]' -target=module.lambda_task_state_change_handler -target=aws_ssm_parameter.telebot_base_env`.
   Never a blanket apply (see the NAT section above).
4. `telebot-deploy` with `passivbot_revisions=latest`.
5. Only templates stamped `config_version: v9.x` will launch on it; every
   existing bot keeps its line until its config is switched.

Memory: size each line from its own observed RSS. The v8 entry starts as a copy
of v7's 400 MB -- measure after the first v8 bot start and adjust.

### Retiring a line

Only once no bot config targets it (check `config_version` across
`<user_id>/<bot_id>/<bot_id>.json`). Remove the map entry and apply. Deregistering
a task definition does NOT stop tasks already running on it; they just can no
longer be (re)launched -- so drain first.

## ECR repositories (module.ecr)

> ⚠️ **STATE/CODE COUPLING:** the live dev state already references
> `module.ecr` (the migration below was performed). The `module.ecr` code must
> be present in **any** checkout used for `terraform apply`, or terraform will see
> `module.ecr.*` in state but not in config and try to **destroy/recreate** the
> repos (telebot has `force_delete=true`; passivbot-live holds the live trading
> image). **Merge this branch before applying from `main`.**

Both image repos are managed by `module.ecr`:
- `telebot` → `scalable-cluster-dev-telebot` (scan-on-push, `force_delete=true`).
- `passivbot_v741` → `passivbot-live` (`scan_on_push=false`, `force_delete=false` —
  matches the live repo so the live trading image is never auto-deleted). The map
  key keeps the legacy `passivbot_v741` name on purpose: renaming it would plan a
  destroy/recreate of the imported `passivbot-live` repo. It is decoupled from the
  task-def family (now version-agnostic `…-passivbot`) and the image tag.

Both **pre-existed** and were adopted into state **without recreation** (done
2026-06-20):
```
terraform state mv 'aws_ecr_repository.telebot' 'module.ecr.aws_ecr_repository.this["telebot"]'
terraform import 'module.ecr.aws_ecr_repository.this["passivbot_v741"]' passivbot-live
```
`terraform plan -target=module.ecr` then showed `0 to destroy` (only a benign
in-place tag addition on passivbot-live). If you ever rebuild state from scratch,
re-run those two commands. **Never** let terraform recreate these repos.

The passivbot image is composed as
`module.ecr.repository_urls["passivbot_v741"]:${var.passivbot_engines[<major>].image_tag}`. To
ship a new passivbot build: push the image to `passivbot-live` under a new tag, set
that line's `image_tag` in `passivbot_engines` (tfvars), scoped `terraform apply` (registers a new task-def
revision), then run **telebot-deploy** (the passivbot sync rule above).

> NOTE: `terraform plan/apply/import` here evaluates the lambda's `archive_file`
> data source, which needs `target/lambda/task_state_change_handler/bootstrap` to
> exist (built separately). Build the lambda first, or the command errors on a
> missing file.

## Lambda (task-state-change-handler) deploy

The `task_state_change_handler` Lambda ships code out-of-band, like telebot — via the
**lambda-deploy** GitHub Actions workflow (manual `workflow_dispatch`), NOT terraform.
It builds the bootstrap through the devcontainer's `lambda-export` Docker stage
(`rust:1.89-bullseye`, glibc 2.31 < AL2023 2.34) and ships it with
`aws lambda update-function-code`. It never touches the env S3 state, the backend
lock, or the NAT.

- **One-time bootstrap** (creates the deploy role + wires the secret):
  ```
  AWS_PROFILE=dev terraform apply \
    -target=aws_iam_role.gh_lambda_deploy \
    -target=aws_iam_role_policy.gh_lambda_deploy
  ```
  Then set GitHub secret `AWS_LAMBDA_DEPLOY_ROLE_ARN` from the
  `lambda_task_state_change_gh_deploy_role_arn` output. As with any apply here, the
  lambda `archive_file` needs `target/lambda/task_state_change_handler/bootstrap` to
  exist — build it first.
- **Ship new lambda code:** run the **lambda-deploy** workflow. It verifies CodeSha256
  and smoke-invokes a benign non-ECS event (ignored → 200, never launches a task).
- **Drift:** `aws_lambda_function.this` carries `ignore_changes = [source_code_hash]`,
  so `terraform apply` does NOT revert CI-shipped code. To deploy lambda code through
  terraform in an emergency, `-replace` the function.

## Recovery

- **telebot down, egress fine:** just re-run **telebot-deploy** (no NAT impact).
  This is the default fix — do NOT revert+apply.
- **Reverting the decoupling commit** re-changes `user_data` → a **second** NAT
  replacement + egress blip. Only do this in a maintenance window, rarely.

## Recommended

Add a "telebot container down" alarm (e.g. on the absence of telebot logs / a
heartbeat) so a forgotten `telebot-deploy` step surfaces instead of failing silent.
