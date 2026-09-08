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

Procedure **without** a standby NAT — this is the fallback, and it costs the
bots several minutes of blackholed egress. Prefer the standby procedure in the
next section, which costs seconds instead.

1. Pause/*quiesce* trading (stop bots; nothing should need egress).
2. `terraform apply` (scoped if possible, e.g. `-target=module.network.aws_instance.nat`
   plus the `aws_ssm_parameter.telebot_base_env`). This rebuilds the NAT and
   (re)publishes base-env.
3. **Immediately** run the **telebot-deploy** workflow (tag `latest`,
   `passivbot_revisions=latest`). This writes the env file and brings telebot up.
4. Verify telebot is running and egress works before resuming trading.

> The **first** application of the config-decoupling refactor is exactly this:
> it changes `user_data`, so it triggers one NAT rebuild. Treat it as the above.

## Rebuilding the NAT without an egress outage (standby NAT)

`terraform.tfvars` carries two switches that put a throwaway NAT in front of the
rebuild:

| Variable | Steady state | Purpose |
|---|---|---|
| `nat_standby_enabled` | `false` | Whether `module.network.aws_instance.nat_standby` exists (a `t4g.nano`, ~$0.0042/hr, billed only while it is up) |
| `nat_egress_active` | `"primary"` | Which NAT carries the private default route **and** the EIP: `"primary"` or `"standby"` |

The standby runs the plain `nat-userdata-al2023.sh` — NAT only, **no telebot, no
app config** — so it never inherits the churn that forces the primary to be
replaced. It is tagged `…-nat-standby`, deliberately not `nat-instance`, so the
telebot-deploy role (which scopes `ssm:SendCommand` by `Name = nat-instance`)
can never land a deploy on it.

**Edit these in `terraform.tfvars`, never pass them with `-var`.** Every apply in
the window has to agree on them; a scoped apply that forgets `-var
nat_egress_active=standby` re-points the default route at the primary NAT that
same apply is busy destroying — the exact outage this is meant to avoid.

### What this does and does not cover

- **Covered:** trading egress. The bots keep their route out for the whole
  rebuild, through the same whitelisted address.
- **Not covered:** telebot. It lives on the primary NAT and stays **down** from
  the moment that instance is replaced until `telebot-deploy` writes
  `/etc/telebot/telebot.env` on the new host. Auto-restart (the lambda) is
  unaffected — it does not run on the NAT.
- **Residual gap:** the exchange API keys are IP-whitelisted, so the EIP has to
  move with the route. Moving it is a disassociate + associate, which leaves a
  **few seconds** where egress exits via a non-whitelisted address and the
  exchange rejects the call — passivbot retries through it. That happens twice
  per window (out to the standby, back to the primary). Seconds of rejected
  calls, versus minutes of a black hole.

### Procedure

1. **Bring the standby up, without touching the live path.** In
   `terraform.tfvars` set `nat_standby_enabled = true`, leave
   `nat_egress_active = "primary"`:
   ```bash
   AWS_PROFILE=dev terraform apply -target=module.network.aws_instance.nat_standby
   ```
   Expected: `1 to add, 0 to change, 0 to destroy`. Live traffic is untouched —
   the route and the EIP have not moved.

2. **Verify the standby actually forwards before trusting it with the bots.**
   It is in the public subnet with a public IP, so SSM reaches it directly:
   ```bash
   STANDBY=$(terraform output -json network | jq -r .nat_standby_instance_id)
   AWS_PROFILE=dev aws ssm send-command --region ap-northeast-1 \
     --instance-ids "$STANDBY" --document-name AWS-RunShellScript \
     --parameters 'commands=["cloud-init status --wait","sysctl net.ipv4.ip_forward","iptables -t nat -S POSTROUTING"]'
   ```
   Read the result with `aws ssm get-command-invocation --command-id <id>
   --instance-id "$STANDBY"`.
   **Do not proceed** unless `cloud-init status` is `done`, `ip_forward = 1`,
   and `POSTROUTING` carries a `-j MASQUERADE` rule.

3. **Flip egress onto the standby.** Set `nat_egress_active = "standby"`:
   ```bash
   AWS_PROFILE=dev terraform apply \
     -target=module.network.aws_route.private_nat \
     -target=module.network.aws_eip_association.nat
   ```
   Expected: the route is an **in-place** update (one atomic `ReplaceRoute`) and
   the EIP association is replaced. This is the few-second gap.

4. **Confirm the bots are out through the standby**, on the standby host:
   `curl -s https://checkip.amazonaws.com` must return the EIP
   (`terraform output -json network | jq -r .nat_eip_public_ip`), and
   `iptables -t nat -L POSTROUTING -n -v` must show climbing counters — that is
   the private subnet's traffic actually traversing it.

5. **Now do the real NAT change.** The primary is rebuilt while the bots keep
   trading through the standby:
   ```bash
   AWS_PROFILE=dev terraform apply \
     -target=module.network.aws_instance.nat \
     -target=aws_ssm_parameter.telebot_base_env
   ```

6. **Bring telebot back.** Wait for the new primary's cloud-init, then run the
   **telebot-deploy** workflow (`tag=latest`, `passivbot_revisions=latest`).
   Verify telebot responds in Telegram.

7. **Flip back.** Set `nat_egress_active = "primary"` and re-run the step-3
   apply. Same few-second gap; confirm the EIP is back on the primary
   (`terraform output -json network`).

8. **Destroy the standby — do not skip this.** Set
   `nat_standby_enabled = false`:
   ```bash
   AWS_PROFILE=dev terraform apply -target=module.network.aws_instance.nat_standby
   ```
   Expected: `0 to add, 0 to change, 1 to destroy`. Commit the tfvars back to the
   steady state (`false` / `"primary"`) so the next apply from a clean checkout
   does not resurrect it — or, worse, find the route pointed at an instance that
   no longer exists.

### If the rebuild goes wrong

Egress is already on the standby, so there is no rush: the bots are trading.
Leave `nat_egress_active = "standby"` and fix the primary at your own pace. The
standby is a `t4g.nano` running the same NAT rules, so it is a fine place to sit
for hours — it just costs pennies and leaves telebot down.

The one state to never leave behind is `nat_standby_enabled = false` while
`nat_egress_active = "standby"`; the module rejects that combination at plan
time rather than letting an apply delete the instance the default route points
at.

## Gateway endpoints: what does NOT go through the NAT

`module.network` attaches S3 and DynamoDB **gateway** endpoints to the private
route table. They are free, have no ENI, and are just prefix-list routes, so the
private subnets reach those two services without touching the NAT at all. That
covers bot config reads/writes, and -- because ECR serves image layers from S3
in-region -- every task's image pull.

What still needs the NAT: the exchange traffic, and the ECR / SSM / CloudWatch
Logs **APIs**. Those are only available as interface endpoints, which are billed
per hour per AZ, so they stay on the NAT.

Two things to know before touching them:

- **Adding or removing an endpoint rewrites the private route table.** In-flight
  S3/DynamoDB connections break at that moment. Bots read their config at start,
  so in practice this is a non-event -- but do not do it in the same apply as
  anything else time-critical.
- **No endpoint policy is set, so the default full-access policy applies. Do not
  narrow it casually.** ECR image pulls resolve to S3 buckets owned by AWS, not
  by this account; a policy scoped to the bot-config bucket would break every
  task launch with an opaque pull error.

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
-> `local.td_passivbot_by_engine` = `7=<arn>,8=<arn>` (plus `8rs=<arn>` per
"pb-runner runtime" below).

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

### pb-runner runtime (`rs`): the pure-Rust image of a line

A line can carry a second image: the pure-Rust runner (pb-runner repo), which
embeds the same passivbot engine crate and speaks the same container contract
(env `BUCKET`/`USER_ID`/`BOT_ID`, downloads `<user>/<bot>/<bot>.json` +
`api-keys.json` from S3 itself, exit codes 10/20/21/22). Which image a bot
launches on is a **per-bot attribute**, `runtime` on the bot row (`py`
default, `rs`), set in Telegram with `/runtime <bot_id> rs` (back with
`/runtime <bot_id> py`, shown by State and `/runtime <bot_id>`). The attribute
is read at launch, so a change applies on the next Run; rollback is flipping it
back and restarting. Both launchers (Run and the lambda auto-restart) route on
`(config line, runtime)`, and a bot set to `rs` whose line has no `rs` entry
gets a user-facing refusal (at `/runtime`, at "Choose config", and at Run)
rather than silently launching the Python image.

Table key syntax: `<major>[rs]` -- `8` is the Python image of line 8, `8rs`
the pb-runner image of line 8. `var.passivbot_engines` keys are the table keys.

1. Build + push the image from the **pb-runner repo**: its CodeBuild project
   is the analogue of `deploy/passivbot-image/buildspec.yml` here
   (`deploy/buildspec.yml` there; env `ENGINE=engine-v8`,
   `IMAGE_TAG=8-v8.1.0-arm64`, `ECR_REPO=pb-runner`). The ECR repo is
   `module.ecr` key `pb_runner` (name `pb-runner`), applied like the other
   repos: `terraform apply -target='module.ecr'`.
2. Uncomment the `"8rs"` entry in `passivbot_engines` (`terraform.tfvars`):
   `image_repo = "pb_runner"`, `family_suffix = "-v8-rs"`, `command =
   ["--live"]` -- pb-runner only trades with `--live`; without it it plans and
   logs. Memory starts at a placeholder (96 MiB, target <= 64) -- measure after
   the first `rs` bot start and adjust.
3. Get the new binaries live BEFORE `8rs` enters the table: merge to main
   (telebot-build pushes the new `:latest` image), then run `lambda-deploy.yml`.
   The new parser reads the old table (`7=…,8=…`) fine, so this step is safe on
   its own; the reverse is not: an old lambda binary given a table with `8rs`
   fails at config load (`EngineTaskDefinitions::parse` on key `8rs`) and
   never boots, taking auto-restart down silently. Step 4's scoped apply bakes
   the table into the lambda env, so the binary must already be there. Note
   this ordering is the OPPOSITE of "First apply of the per-engine split":
   there env and binary changed shape together, so they had to be
   back-to-back; here only the table gains a key the new binary already
   understands.
4. Scoped apply, as for a new line:
   `terraform apply -target='module.passivbot_task["8rs"]' -target=module.lambda_task_state_change_handler -target=aws_ssm_parameter.telebot_base_env`.
5. `telebot-deploy` with the post-merge image tag (`latest`, or that merge's
   SHA) and `passivbot_revisions=latest` (pins accept the same keys: `8rs=3`).
   An older telebot image fails the same way the old lambda does once the
   families contain `8rs`.
6. Move one bot: `/runtime <bot_id> rs`, Stop, Run; watch
   `/ecs/<project>-<env>/passivbot-v8-rs`. Roll back with `/runtime <bot_id> py`
   + restart. Every other bot keeps `py` until moved.

Rollback of the table itself: re-comment `8rs` in `terraform.tfvars` and
re-run the step-4 apply to return the table to the old shape. Only valid while
no bot's `runtime` is still `rs` -- such a bot would be refused at Run and by
the auto-restart, so flip every `rs` bot back to `py` first.

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
(`rust:1.89-bookworm`, no symbol above GLIBC_2.34 = AL2023's glibc) and ships it with
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
