---
name: pbtb-deploy
description: Roll a change out in the right order: telebot, the restart or collector lambda, passivbot task definitions / engine lines, passivbot images, the site. Use on deploy / apply / rollout, when a merged change touches terraform, a workflow, an env variable or a Dockerfile, or adds an engine line.
---

# pbtb deploy

The RUNBOOK (`terraform/envs/dev/RUNBOOK.md`) is the source of truth for
*why*; this skill is the executable order. Before and after every rollout run
`python scripts/ops/pbtb_ops.py deploy-audit` — its `findings` list is the
definition of "in sync".

## Which mechanism for which component

| Component | Mechanism | Account / tool |
|---|---|---|
| telebot binary | `gh workflow run telebot-deploy.yml --ref main -f tag=latest -f passivbot_revisions=latest` (image built automatically by `telebot-build` on push to main) | the local agent App (docs/agents.md § Local agents) |
| restart lambda code | `gh workflow run lambda-deploy.yml --ref main` | local agent App |
| collector lambda code | `gh workflow run daily-pnl-snapshot-deploy.yml --ref main` | local agent App |
| candle collector lambda code | `gh workflow run lambda-deploy.yml --ref main -f target=hl-candle-collector` | local agent App |
| lambda env / IAM, task definitions, base-env | `terraform -chdir=terraform/envs/dev apply -target=… -auto-approve` — **always scoped**; a blanket apply reaches the NAT instance, whose replacement blackholes all trading egress (AGENTS.md invariant) | `AWS_PROFILE=dev` |
| passivbot image | `python scripts/build_passivbot_image.py --tag vX.Y.Z-arm64 --no-wait`, then `pbtb_ops.py codebuild-log <build-id>` | dev profile; source at `E:/projects/passivbot` checked out at that tag |
| web console (site) | `gh workflow run pages-publish.yml --ref main` (also on every push to main touching `site/`; the showcase is not in the build, the pages read it from the showcase CDN, so a switch or a collector run needs no publish) | local agent App |

## The coupling rule (read twice)

Two launchers hold the passivbot task-definition table from different sources:
the lambda gets it baked at `terraform apply`, telebot gets it from
`telebot-deploy`. **Every passivbot apply is followed by a telebot-deploy.**

When a change alters the *shape* of the env a binary reads (a renamed or new
`APP__*` key), the old binary with the new env — or the new binary with the old
env — fails at config load. Sequence such changes back-to-back, and know the
window: between the apply and `lambda-deploy`, OOM auto-restarts silently stop
and a Restart leaves its bot Enabled · Stopped.

```
merge → (telebot-build runs) → scoped terraform apply → telebot-deploy → lambda-deploy
```

A new optional attribute on the bot row is erased by any deployed binary that
saves the whole row (`BotRepository::save` is a `PutItem` of the fields that
binary knows). Every binary whose use cases call it (grep `bots.save(` under
`src/usecase/` and follow the use case to its composition root; today telebot and
`-f target=mcp-http`) ships before the attribute's first write.

A new *value* of an attribute every reader parses (an `Exchange` variant, a
`runtime`) is the reverse: an old binary reads the row as `CorruptRecord`, and a
`find_all` reader (the collector) then fails its whole run. Every reader of bot
rows — `daily-pnl-snapshot-deploy`, `lambda-deploy`, telebot, `-f
target=mcp-http` — ships before any writer can produce the value.

If `lambda-deploy` fails mid-window, mitigate immediately: put the key the old
binary needs back with `aws lambda update-function-configuration --environment
file://<full map>` (it replaces the whole map — fetch it first), smoke, fix the
build, deploy, then `terraform apply -target=module.lambda_task_state_change_handler`
to remove the key again. Verify after each step with `smoke-lambda task-state`.

## Before you apply

1. Read-only plan with the exact `-target` set you will apply. Quote the
   `Plan: X to add, Y to change, Z to destroy` line. **Z must be 0** unless the
   destroy is the point. Task definitions "updated in-place" keep their revision;
   a "must be replaced" on a family running live bots is a stop-and-think.
2. If the plan errors with *Moved resource instances excluded by targeting*,
   add the listed addresses — the migration apply for a `for_each` conversion
   must include the whole module.
3. An env map shown as `(known after apply)` is the ARN of a resource being
   created in the same apply, not a dropped key — but confirm the keys after.
4. `-target=aws_ssm_parameter.telebot_base_env` never travels alone:
   `local.passivbot_families` references `module.passivbot_task`, so the
   passivbot task definitions ride along with base-env. Their presence in the
   plan is expected; what each is changing *to* is not — a stale image tag in
   `terraform.tfvars` once turned this plan into a rollback of the rs line to a
   build six hours older. Read every passivbot line before typing yes.

## After you apply / deploy

- `python scripts/ops/pbtb_ops.py deploy-audit` → no findings.
- `python scripts/ops/pbtb_ops.py smoke-lambda task-state` → 200, no FunctionError.
- telebot: `pbtb_ops.py telebot-logs --since 10m --env-dump` → container up,
  `restarts=0`, env has the table, log shows `Starting Telegram bot...`.
- Running bots: `bot-status all --memory` — the count of RUNNING tasks did not
  change, and no task is above 85% of its limit.

## Rolling back

Who may roll back without asking is docs/governance.md § Autonomy tiers; this
is the order.

1. **Before every deploy**, keep the baseline: `deploy-audit` (its findings,
   and the `telebot running sha` line, which is the rollback point) and
   `bot-status all` (the RUNNING count). A check "fails" only against this
   baseline: a standing finding (`:latest` behind main) is not a regression.
2. **When a check in "After you apply / deploy" fails**, file an Incident first
   (`gh issue create --label incident,source:agent`, at *Diagnose only*, with
   the failing output): the revert PR carries `refs #<incident>`, never
   `closes`, since `contract` fails a PR closing an issue below a merge tier.
3. `python scripts/ops/pbtb_ops.py rollback-plan <pr> --running-sha <sha from 1>`
   prints the revert range and the redeploys, or exits 2 with why an agent may
   not do it alone: a revert range it cannot verify, terraform, a code-owned
   path, a stored-shape or env-shape change, a ticked invariant, a revert of a
   revert, or a telebot pin that would take other image changes back with it.
   A rollback is a rollout to an older binary: the coupling rule above holds
   for every commit between the pinned sha and main, not only the reverted
   PR's, and state a binary reads and writes back (the collector's `_state/`)
   is a stored shape like a bot row.
4. **telebot first, without waiting for the merge**: `telebot-deploy -f
   tag=<recorded sha>`. Never `tag=latest`: `telebot-build` skips a source
   hash it has built, so after a revert `:latest` would still be the bad image.
   The deploy re-points `:latest` at the recorded sha, and `deploy-audit` then
   reports `:latest` behind main until the next telebot change; expected.
5. The revert, as `rollback-plan` prints it, through pbtb-ship (gate, review,
   verdict line, PR, auto-merge). After it merges: the collector redeploy if
   the plan lists it; the site rides the push.
6. Re-run the checks against the baseline and put the result on the Incident.
   A lambda the plan names as the owner's call is left for the owner.

## Rebuilding the NAT (sole egress + telebot host)

Any change to the NAT `user_data` or AMI replaces `module.network.aws_instance.nat`
— the one host all trading egress leaves through. Never let that land in a
blanket apply.

Since the standby NAT exists, the outage is optional. Set
`nat_standby_enabled = true` in `terraform.tfvars` (**edit the file — never pass
these with `-var`**, or a later scoped apply in the window routes egress back
onto the instance it is destroying), then:

```
apply -target=module.network.aws_instance.nat_standby     # 1 to add; live path untouched
  → verify cloud-init done + ip_forward=1 + MASQUERADE present, over SSM
  → nat_egress_active = "standby"; apply -target=…aws_route.private_nat -target=…aws_eip_association.nat
  → apply -target=module.network.aws_instance.nat -target=aws_ssm_parameter.telebot_base_env
  → telebot-deploy                                        # telebot is DOWN until this runs
  → nat_egress_active = "primary"; re-apply the route + EIP pair
  → nat_standby_enabled = false; apply -target=…aws_instance.nat_standby   # 1 to destroy
```

The exchange API keys are IP-whitelisted and the EIP has to move with the route,
so each flip has a **few-second** window where egress leaves via a
non-whitelisted address and the exchange **rejects** (does not time out) — twice
per window. That is the cost; the alternative is minutes of blackholed egress.

Do not skip the teardown: leaving the standby up bills a `t4g.nano` forever, and
leaving `nat_egress_active = "standby"` in a merged tfvars points the default
route at an instance the next clean apply will not create. Full step-by-step,
with the expected plan at each step: RUNBOOK, "Rebuilding the NAT without an
egress outage".

## Adding a passivbot engine line (e.g. v9)

1. Build + push the image (CodeBuild). Task-def registration does not validate
   that the image exists; a launch would fail at pull.
2. Add `"9" = { image_tag = "v9.0.0-arm64", memory = <measured MiB> }` to
   `passivbot_engines` in `terraform.tfvars`. Never set `family_suffix = ""` —
   that is reserved for the line that inherited the original family.
3. `terraform apply -target='module.passivbot_task["9"]' -target=module.lambda_task_state_change_handler -target=aws_ssm_parameter.telebot_base_env`
4. `telebot-deploy` (`passivbot_revisions=latest`). No code change and no
   `lambda-deploy` are needed: both binaries read the table from env.
5. Nothing launches on the new line until a config stamped `config_version: v9.x`
   is chosen for a bot and that bot is Stop/Run.

Memory per line: size from the line's own observed RSS (`bot-status --memory`
after warm-up and after a day), not by copying another line's number. A task on
an older revision that already exceeds the latest revision's limit will OOM on
its next restart — raise the limit before that restart, not after.
