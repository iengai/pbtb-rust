# Telebot deploy

The telebot (pbtb-rust control bot) runs as a container co-located on the dev NAT instance (tag `Name=nat-instance`). Two GitHub Actions workflows manage its lifecycle: `telebot-build` produces and pushes the image to ECR, and `telebot-deploy` lands the image and its config on the NAT host. Neither uses Terraform, and a normal telebot deploy has **no NAT impact** — app/image/passivbot churn never touches the NAT instance lifecycle.

## Config model

Responsibility for telebot config is split so that app churn never reaches the NAT lifecycle:

- **NAT `user_data`** carries only NAT/host bootstrap — **zero app config**. It installs docker, `run-telebot.sh`, and the `telebot.service` unit.
- **`run-telebot.sh`** reads all runtime config from `/etc/telebot/telebot.env` and fetches the Telegram token from SSM (`/scalable-cluster/dev/telebot/teloxide-token`, a SecureString) at container start. Until that env file exists it exits 1 and systemd retries every 30s.
- **Terraform** publishes the stable, non-secret config to the `base-env` SSM `String` parameter (`/scalable-cluster/dev/telebot/base-env`) on every apply, so infra renames propagate without rebuilding the NAT.
- **`telebot-deploy`** resolves the passivbot task-def ARN, composes `/etc/telebot/telebot.env` (base-env + that ARN), writes it to the NAT via SSM, and restarts telebot.

Terraform owns infra and stable config; telebot-deploy owns the app config that lands on the host.

The token **never** lives on disk or in `base-env` — it is fetched live from SSM at container start. `base-env` values are plain, single-line, metacharacter-free `KEY=value` tokens (no spaces, quotes, `$`, or backticks): the host parses them with `grep`/`cut` and `docker --env-file`, neither of which shell-quotes. The deploy workflow fails closed if `base-env` is empty or ever contains `TELOXIDE_TOKEN`.

`base-env` contents (rendered by Terraform in `terraform/envs/dev/telebot.tf`, `local.telebot_base_env`):

```
RUST_LOG=info
APP__DYNAMODB__REGION=<region>
APP__DYNAMODB__TABLE_NAME=scalable-cluster-dev-bots
APP__S3__REGION=<region>
APP__S3__BUCKET_NAME=<bucket>
APP__S3__ENDPOINT_URL=https://s3.<region>.amazonaws.com
APP__ECS__REGION=<region>
APP__ECS__CLUSTER_ARN=arn:aws:ecs:<region>:<account>:cluster/scalable-cluster-dev-cluster
APP__ECS__TD_PASSIVBOT_V741_CONTAINER_NAME=<container-name>
TELEBOT_REGION=<region>
TELEBOT_ECR_REGISTRY=<account>.dkr.ecr.<region>.amazonaws.com
TELEBOT_IMAGE=<telebot-repo-url>:<tag>
TELEBOT_TOKEN_PARAM=/scalable-cluster/dev/telebot/teloxide-token
TELEBOT_MEMORY=<memory-cap>
```

The one value `telebot-deploy` appends at deploy time is `APP__ECS__TD_PASSIVBOT_V741_ARN` (the resolved passivbot task-def ARN).

## OIDC roles

Both workflows authenticate to AWS via GitHub OIDC (no static keys). The roles are defined in `terraform/envs/dev/telebot.tf` and trust only jobs on `refs/heads/main`. Their ARNs are exposed as Terraform outputs and must be set as GitHub repo secrets:

| Secret | Terraform output | Role | Permissions |
| --- | --- | --- | --- |
| `AWS_BUILD_ROLE_ARN` | `telebot_gh_build_role_arn` | `scalable-cluster-dev-telebot-gh-build` | ECR auth + push to the telebot repo |
| `AWS_DEPLOY_ROLE_ARN` | `telebot_gh_deploy_role_arn` | `scalable-cluster-dev-telebot-gh-deploy` | ECR re-tag, `ec2:DescribeInstances`, `ssm:SendCommand` (scoped to `tag:Name=nat-instance` + the `AWS-RunShellScript` document), read command results, `GetParameter` on base-env, `ecs:DescribeTaskDefinition` |

Region for both: `ap-northeast-1`. ECR repo: `scalable-cluster-dev-telebot`.

## telebot-build

File: `.github/workflows/telebot-build.yml`. Builds the `linux/arm64` image and pushes it to ECR. It does not touch the running NAT.

**Triggers:**
- Push to `main` touching any of `src/**`, `Cargo.toml`, `Cargo.lock`, `.devcontainer/Dockerfile`, or `.github/workflows/telebot-build.yml` (a docs-only push never triggers it).
- Manual `workflow_dispatch`, with a `force` boolean input to rebuild even when an image for the current source already exists.

**Content-hash gate.** A cheap `check` job (standard `ubuntu-latest` runner, no compile) hashes the build-relevant files (`src/**`, `Cargo.toml`, `Cargo.lock`, `.devcontainer/Dockerfile`, `.dockerignore`) into a tag `src-<hash>`. If `force` is not set and ECR already has an image with that tag (`aws ecr describe-images`), the build is skipped. This avoids redundant compiles on manual re-runs or mixed pushes. If `hashFiles` matches nothing, the job fails rather than build a bogus tag.

**Build job.** Runs only when `needs_build == true`, on a **native arm64 runner** (`ubuntu-24.04-arm`) — this Rust project is never cross-built via QEMU. It builds the `runtime` target of `.devcontainer/Dockerfile` with `BIN_NAME=pbtb-rust`, restoring a cargo registry + target cache across runs so only changed crates recompile.

**Tags pushed** (all three, to `scalable-cluster-dev-telebot`):
- `src-<hash>` — the source-hash tag that drives the skip check
- `<git-sha>` — immutable per-commit tag (used for rollback)
- `latest`

## telebot-deploy

File: `.github/workflows/telebot-deploy.yml`. Deploys an image already in ECR onto the NAT via SSM. No build, no Terraform, no NAT replacement. Manual `workflow_dispatch` so deploying to the trading-adjacent host is always deliberate.

**Inputs:**
- `tag` — telebot ECR image tag to deploy: a git SHA, or `latest` (default).
- `passivbot_revision` — passivbot task-def revision telebot should launch: a number, or `latest` (default) for the current active revision.

**Steps** (single `deploy` job on `ubuntu-latest`):

1. **Point `:latest` at the requested tag** — only when `tag != latest`. The NAT's `run-telebot.sh` always pulls `:latest`, so to deploy a specific tag the workflow re-points `:latest` at it in ECR first (`batch-get-image` → `put-image`). Fails if the tag is not found.
2. **Resolve passivbot task-def ARN** — describes `scalable-cluster-dev-passivbot-v741` (at `:<revision>` if a number was given, else the active revision) and captures the full revisioned ARN. Resolving the ARN here, rather than in `user_data`, is what decouples the NAT from passivbot bumps while keeping the launch precisely pinned.
3. **Push `telebot.env` and restart on the NAT via SSM** — reads `base-env` from SSM, fails on the runner before overwriting the host's good env file if it is empty/`None` or contains `TELOXIDE_TOKEN`, then composes the env file as base-env + `APP__ECS__TD_PASSIVBOT_V741_ARN=<resolved ARN>`. The whole remote script (write `/etc/telebot/telebot.env` with mode 600 under `/etc/telebot` mode 700, `systemctl restart telebot`, then a health loop) travels as one base64 blob so no shell metacharacters reach the SSM command JSON. It finds the running NAT by `tag:Name=nat-instance`, sends `AWS-RunShellScript`, and waits for the result.

**Health check.** The remote script polls up to 12 times at 5s intervals for a running `telebot` container (`docker ps --filter name=telebot --filter status=running`). On failure it prints `telebot-not-running` plus the last service status and container logs and exits 1; on success it prints `telebot-up`. The job also asserts the SSM invocation `Status` is `Success`.

## passivbot task-def sync rule

Two consumers resolve the passivbot task-def from **different** sources, and they can diverge:

- **Lambda** (auto-restart on OOM) uses the revisioned ARN baked at `terraform apply` (`envs/dev/main.tf` → lambda `APP__ECS__TD_PASSIVBOT_V741_ARN`).
- **Telebot** (user "Run bot") uses the ARN resolved by `telebot-deploy` at deploy time.

If you bump passivbot via apply but do not re-run telebot-deploy, the lambda restarts at the new revision while telebot still launches the old one (or vice versa on a telebot-only rollback).

**Rule: every passivbot bump apply must be followed by a telebot-deploy.** The default `passivbot_revision=latest` matches the just-applied revision. A deliberate telebot-only rollback (`passivbot_revision=<n>`) knowingly diverges from the lambda until the next apply.

## Normal operations (no NAT impact)

- **Ship a new telebot build:** push to `main` → `telebot-build` builds and pushes → run **telebot-deploy** (`tag=latest`). Re-pulls the image, rewrites the env file, restarts.
- **Roll telebot back to an older image:** run **telebot-deploy** with `tag=<git-sha>`. The deploy re-points `:latest` at that SHA before restarting.
- **Bump the passivbot version:** edit `var.passivbot_v741_image_tag` → `terraform apply` (registers a new task-def revision; the lambda picks it up at apply) → **then run telebot-deploy** (`passivbot_revision=latest`), per the sync rule above.

## Recovery

- **telebot down, egress fine:** just re-run **telebot-deploy** (no NAT impact). This is the default fix — do not revert and apply.

For the cases that *do* replace the NAT (Terraform changes that alter `user_data`), see the dev runbook: `../../terraform/envs/dev/RUNBOOK.md`.
