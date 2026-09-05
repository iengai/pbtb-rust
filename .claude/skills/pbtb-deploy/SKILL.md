---
name: pbtb-deploy
description: How to roll anything out in this trading system — telebot, the restart lambda, the collector lambda, passivbot task definitions / engine lines, passivbot images, the return-curve site — and in which order. Use it whenever the user says 部署 / 上线 / deploy / roll out / apply / "把这个改动推上去", when a merged change touches terraform, a workflow, an env variable, or a Dockerfile, and when adding a new passivbot engine line. Rolling these in the wrong order has taken the auto-restart lambda offline before; this skill encodes the order and the checks.
---

# pbtb deploy

The RUNBOOK (`terraform/envs/dev/RUNBOOK.md`) is the source of truth for
*why*; this skill is the executable order. Before and after every rollout run
`python scripts/ops/pbtb_ops.py deploy-audit` — its `findings` list is the
definition of "in sync".

## Which mechanism for which component

| Component | Mechanism | Account / tool |
|---|---|---|
| telebot binary | `gh workflow run telebot-deploy.yml --ref main -f tag=latest -f passivbot_revisions=latest` (image built automatically by `telebot-build` on push to main) | `gh auth switch --user iengai` (only that account has `workflow` scope); switch back after |
| restart lambda code | `gh workflow run lambda-deploy.yml --ref main` | iengai |
| collector lambda code | `gh workflow run daily-pnl-snapshot-deploy.yml --ref main` | iengai |
| lambda env / IAM, task definitions, base-env | `terraform -chdir=terraform/envs/dev apply -target=… -auto-approve` — **always scoped**; a blanket apply bundles the latest-AMI bump and recycles the single ECS host, killing live bots | `AWS_PROFILE=dev` |
| passivbot image | `python scripts/build_passivbot_image.py --tag vX.Y.Z-arm64 --no-wait`, then `pbtb_ops.py codebuild-log <build-id>` | dev profile; source at `E:/projects/passivbot` checked out at that tag |
| return-curve site | `gh workflow run pages-publish.yml --ref main` (also daily) | iengai |

## The coupling rule (read twice)

Two launchers hold the passivbot task-definition table from different sources:
the lambda gets it baked at `terraform apply`, telebot gets it from
`telebot-deploy`. **Every passivbot apply is followed by a telebot-deploy.**

When a change alters the *shape* of the env a binary reads (a renamed or new
`APP__*` key), the old binary with the new env — or the new binary with the old
env — fails at config load. Sequence such changes back-to-back, and know the
window: between the apply and `lambda-deploy`, OOM auto-restarts silently stop.

```
merge → (telebot-build runs) → scoped terraform apply → telebot-deploy → lambda-deploy
```

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

## After you apply / deploy

- `python scripts/ops/pbtb_ops.py deploy-audit` → no findings.
- `python scripts/ops/pbtb_ops.py smoke-lambda task-state` → 200, no FunctionError.
- telebot: `pbtb_ops.py telebot-logs --since 10m --env-dump` → container up,
  `restarts=0`, env has the table, log shows `Starting Telegram bot...`.
- Running bots: `bot-status all --memory` — the count of RUNNING tasks did not
  change, and no task is above 85% of its limit.

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
