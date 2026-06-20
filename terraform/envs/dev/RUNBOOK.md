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
   `passivbot_revision=latest`). This writes the env file and brings telebot up.
4. Verify telebot is running and egress works before resuming trading.

> The **first** application of the config-decoupling refactor is exactly this:
> it changes `user_data`, so it triggers one NAT rebuild. Treat it as the above.

## Normal operations (no NAT impact)

- **Ship a new telebot build:** push to `main` → `telebot-build` builds+pushes →
  run **telebot-deploy** (`tag=latest`). Re-pulls the image + rewrites env + restart.
- **Roll telebot back to an older image:** telebot-deploy with `tag=<git-sha>`.
- **Bump the passivbot version:** edit `var.passivbot_v741_image` → `terraform apply`
  (registers a new task-def revision; the lambda picks it up at apply) → **then run
  telebot-deploy** (`passivbot_revision=latest`) so telebot also launches the new
  revision. See the divergence rule below.

## passivbot task-def ARN: keep lambda and telebot in sync

Two consumers resolve the passivbot task-def from **different** sources:
- **lambda** (auto-restart on OOM): the revisioned ARN baked at `terraform apply`
  (`envs/dev/main.tf` → lambda `APP__ECS__TD_PASSIVBOT_V741_ARN`).
- **telebot** (user "Run bot"): the ARN resolved by **telebot-deploy** at deploy time.

If you bump passivbot via apply but do **not** re-run telebot-deploy, the lambda
restarts at the new revision while telebot still launches the old one (or vice
versa on a telebot-only rollback). **Rule: every passivbot apply is followed by a
telebot-deploy** (default `passivbot_revision=latest` matches the just-applied
revision). A deliberate telebot-only rollback (`passivbot_revision=<n>`) knowingly
diverges from the lambda until the next apply.

## Recovery

- **telebot down, egress fine:** just re-run **telebot-deploy** (no NAT impact).
  This is the default fix — do NOT revert+apply.
- **Reverting the decoupling commit** re-changes `user_data` → a **second** NAT
  replacement + egress blip. Only do this in a maintenance window, rarely.

## Recommended

Add a "telebot container down" alarm (e.g. on the absence of telebot logs / a
heartbeat) so a forgotten `telebot-deploy` step surfaces instead of failing silent.
