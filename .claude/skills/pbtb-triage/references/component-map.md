# Component map (dev, account 025418542265, ap-northeast-1, `AWS_PROFILE=dev`)

Read the row for the component involved. Every "quirk" here has already cost a
debugging session; they are not hypothetical. How a component is rolled out
lives elsewhere: the `pbtb-deploy` skill for the order and the commands,
`docs/deployment/` for the why; the "Deployed by" column only names the path.

| Component | Runs where | Deployed by | Logs / evidence | Drift to check | Quirks |
|---|---|---|---|---|---|
| **telebot** (Telegram control plane) | systemd docker service `telebot` on the **NAT/EC2 host** (`tag:Name=nat-instance`). **Not in ECS** — no ECS service/task-def exists for it. | `telebot-build` then `telebot-deploy` (docs/deployment/telebot.md) | `pbtb_ops.py telebot-logs --app` (docker logs over SSM) for the application; without `--app` it reads journald, which holds only the wrapper: docker login, image pull, `Starting Telegram bot...`. A `ref: xxxxxxxx` in Telegram is the redaction id logged next to the real error (`interface/telegram/redaction.rs`). | Running image digest vs ECR `:latest`; `telebot.env` has `APP__ECS__TD_PASSIVBOT_BY_ENGINE` and it matches the latest task-def revisions (`deploy-audit` does both). | Any row kept beside the bots in a `user_id#…` partition needs a `<kind>#` sort key (docs/data-model.md, "Row shapes and the readers"); a binary built before `is_bot_row` fails the whole tenant list on a shape it was not taught, so deploy before writing a new one. |
| **passivbot bots** | ECS tasks on `scalable-cluster-dev-cluster`, one per bot, **task-def family per engine line**: `…-passivbot` (v7), `…-passivbot-v8` (v8). Launched by telebot Run and by the restart lambda via `RunTask` with `USER_ID`/`BOT_ID` overrides. | Task-defs by scoped `terraform`, images by CodeBuild (`pbtb-deploy`) | Container logs: `/ecs/scalable-cluster-dev/passivbot` (v7) / `…/passivbot-v8` (v8), stream `passivbot/passivbot-container/<task-id>`. Memory: **Container Insights** (`ECS/ContainerInsights` `MemoryUtilized`, or the `…/performance` log, Type=Task) — `bot-status --memory`. | Task revision vs latest; **usage vs the latest revision's hard limit**; config `config_version` vs the family it runs on. | The ECS container host is **not SSM-managed** (`docker stats` impossible). A task keeps running on a deregistered/older revision until restarted — the limit change only bites at restart. |
| **restart lambda** `task-state-change-handler` | Lambda, triggered by ECS task state-change events. Reads bot desired state (DynamoDB), the bot's config (S3, for the engine line), relaunches OOM-stopped enabled bots behind a CAS start lock. | `lambda-deploy` for code, scoped `terraform` for env/IAM (docs/deployment/lambda.md) | `/aws/lambda/scalable-cluster-dev-task-state-change-handler` (`pbtb_ops.py lambda-logs task-state`). `smoke-lambda task-state` invokes with an ignored event: 200 + no FunctionError proves the binary cold-starts and parses its env. | `CodeSha256` vs the last green `lambda-deploy`; env keys vs what the binary version expects (`deploy-audit`). | **Env and binary are coupled**: a terraform apply that changes env shape must be followed by `lambda-deploy` immediately (and vice-versa); in the window the old binary fails at config load and OOM restarts silently stop. Mitigation pattern: temporarily add the old key back with `update-function-configuration`, then let terraform remove it. |
| **collector lambda** `daily-pnl-snapshot` | Lambda in VPC (egress via NAT EIP — Bybit keys are IP-whitelisted), EventBridge cron 01:00 UTC. | `daily-pnl-snapshot-deploy` (`pbtb-deploy`) | `/aws/lambda/scalable-cluster-dev-daily-pnl-snapshot`. Real run needs a `Scheduled Event` payload; anything else hits the guard and returns. | Chart files in `s3://scalable-cluster-dev-return-charts/charts/` refreshed today? `_state/` present per bot? | Bybit calls only work from the NAT EIP — a local probe returns `retCode 10010 Unmatched IP`; that is expected, not a key problem. |
| **web console (site)** | GitHub Pages, source = **GitHub Actions** (`pages-publish.yml`, on push to main touching `site/` + dispatch). Code and committed template backtests only; return curves reach the browser through `GET /api/v1/bots/{id}/returns` after sign-in. | `pages-publish` (`pbtb-deploy`) | Workflow run logs; `curl https://iengai.github.io/pbtb-rust/templates/index.json`. | Pages `build_type` must be `workflow`. No AWS credentials in the workflow. | The old `gh-pages` branch is unused; pushing to it changes nothing. A stale *curve* is the collector, not the site. |
| **CI (GitHub Actions)** | `telebot-build` (push), `telebot-deploy`, `lambda-deploy`, `daily-pnl-snapshot-deploy`, `pages-publish`, `incident-intake`, `incident-diagnose` (dispatch/schedule/issue). | — | `gh run view <id> --log \| grep …`. OIDC roles per workflow; env-scoped jobs need `environment:<name>` in the role trust. | Secrets present (`gh secret list`); a `workflow_dispatch` workflow must exist on **main** to be dispatchable at all (404 otherwise). | The `iengai` gh account is the default on this machine and the only one with `workflow` scope and push rights; stay on it (`gh auth status`). Builds use `rust:1.89-bookworm` (no symbol above GLIBC_2.34, exactly AL2023's glibc); the builder installs only `ca-certificates` — everything is rustls, do not re-add libssl-dev (its security-suite pool file 404s from GitHub's CDN edge). |
| **CodeBuild** `pbtb-passivbot-image-builder` | arm64 fleet, builds `deploy/passivbot-image/Dockerfile.ecs`. | `scripts/build_passivbot_image.py` | `pbtb_ops.py codebuild-log <build-id>` — the CLI's JSON for this log is often corrupt; the script salvages it. | — | The slim runtime stage has **no compiler**: any Python dep without an aarch64 wheel must be wheel-built in stage 1 (`pip wheel -r requirements-live.txt`). |
| **DynamoDB** `scalable-cluster-dev-bots` | single table: `pk=user_id#<uid>`, `sk=<bot_id>` (bot), `ecs_task_metadata#<bot_id>` (runtime/lock), `config_switch#<bot_id>#<ts>` (timeline). | terraform | `bot-status` renders all three per bot. | — | Condition-expression/CAS changes are only validated against **dynamodb-local** (`cargo test --test botrepository_test` in the dev container); in-memory mocks pass silently. |
| **Bot configs** | `s3://scalable-cluster-dev-bot-configs/<uid>/<bot_id>/<bot_id>.json` (+ `api-keys.json`); templates under `predefined/<id>.json`, opaque ids like `tpl-bzwt9jn2` named by `pbtb.title` (docs/config-transfer.md); an old name in a stored config resolves via `scripts/template_naming.py`. | telebot Choose-config (`ApplyTemplateUseCase`) | `bot-status` shows `config_version`, template, engine line. S3 versioning holds history. | Engine line = `config_version` major; absent → shape (`bot.<side>.risk` ⇒ v8); unparseable → Run refused. | Choosing a config does **not** restart the bot; it takes effect on the next Stop/Run or auto-restart. |

## Error intake (Sentry) and automatic diagnosis

Every binary above reports its `tracing::error!` events to Sentry (org `pbtb`,
project `pbtb-rust`, region `https://de.sentry.io`) when `APP__SENTRY__DSN` is
set; `WARN`/`INFO` lines ride along as breadcrumbs. An event carries the tags
`component` (the binary), `ref_id` (the same id Telegram shows), `release`
(`pbtb-rust@<crate version>`) and `environment`. The `incident-intake`
workflow files one Incident issue per unresolved Sentry issue (labels
`incident` + `source:agent`, body marker `<!-- sentry:<id> -->`), so an issue
with that marker started life in Sentry: open the permalink in the Symptom
block for the full event and its breadcrumbs before reaching for the log
groups. Sentry is the grouped view, not the primary evidence; the log line
with the unredacted chain is still where the component's "Logs / evidence"
column points. `python scripts/ops/sentry_issues.py list` prints what is
unresolved (needs `SENTRY_AUTH_TOKEN`).

The `incident-diagnose` workflow runs this skill's loop as a model with
read-only tools and posts a "🤖 Diagnosis" comment on the issue. What it
can see is the `gh-diagnose` IAM role: `bot-status` (rows without keys),
`deploy-audit` (the NAT host shows as *not probed*: no SSM), `lambda-logs`,
the Sentry event, the checkout. Treat its comment as a first pass with
evidence to re-run, not as the finding; it cannot see the telebot host and
it never runs a trading action. Re-run by hand:
`gh workflow run incident-diagnose.yml -f issue_number=<n>`.

## Tooling quirks (Windows host)

- Prefix AWS CLI calls that take `/aws/...` or `/scalable-cluster/...` paths with
  `MSYS_NO_PATHCONV=1` in Git Bash, or call from Python (`pbtb_ops.py` does).
- The AWS CLI is Python on a cp932 console: run it with `PYTHONUTF8=1`
  (`pbtb_ops.py` does) or it dies mid-output on logs with box-drawing or emoji
  characters and what reaches you looks like corrupt JSON.
- Bash-tool quirks (heredocs, `set -e`, console encoding) are in
  `.claude/CLAUDE.md`, which every Claude Code session loads.
- Docker Desktop drops out intermittently; the dev container `app-node` (and
  `dynamodb-local`) come back with it. Host `cargo` works for check/clippy/test as
  an early signal; `rust-toolchain.toml` pins host and container to one version.
