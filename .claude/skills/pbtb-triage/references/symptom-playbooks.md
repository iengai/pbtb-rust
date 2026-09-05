# Symptom playbooks

Each playbook is the shortest path from the symptom to a confirmed cause. Run the
commands; do not paraphrase them from memory.

## telegram-ref

Telegram replied `❌ Something went wrong while <action>. (ref: 8-hex)`.

1. `python scripts/ops/pbtb_ops.py telebot-logs --grep <ref> --since 2h`
   → the unredacted `ERROR … ref_id="<ref>"` line and its source chain.
2. Classify the logged error:
   - `corrupt persisted record: bot row … did not parse` → a row type the running
     binary does not know. Check `deploy-audit` for a stale telebot, and whether
     anyone wrote new row kinds (`config_switch#`, …) recently. Fix = deploy the
     binary that skips them (`telebot-deploy`), not deleting rows.
   - `DynamoDB … AccessDenied` / `ValidationException` → IAM or a condition
     expression; reproduce with `cargo test --test botrepository_test` against
     dynamodb-local before touching prod.
   - `ecs run_task failed` → see **bot-not-running**.
3. `telebot-logs --env-dump` shows the non-secret env the host actually runs
   with; compare with what the binary version expects (`APP__ECS__TD_PASSIVBOT_BY_ENGINE`
   since #34).

## bot-not-running

1. `python scripts/ops/pbtb_ops.py bot-status <bot_id> --memory` — read
   `enabled`, `runtime`, whether a task exists, its family/revision, and ATTENTION.
2. No task but `runtime=starting` and `enabled=true`: the launch is in flight or
   the lock is stale. Check `telebot-logs --grep <bot_id>` for the Run and
   `lambda-logs task-state --pattern <bot_id>`.
3. Task exists but keeps stopping: read the container log of the LAST stopped
   task (`aws ecs list-tasks --desired-status STOPPED --family <family>` → stream
   `passivbot/passivbot-container/<task-id>` in the family's log group). Exit 137
   with a non-UserInitiated stop code is OOM → compare `bot-status --memory`
   against the family's limit.
4. Run was refused with a `⚠️` message naming an engine line: that is the
   routing gate working — the config's `config_version` targets a line with no
   registered image. Either the template is wrong or `passivbot_engines` needs the
   line (see pbtb-deploy "Adding an engine line").

## no-auto-restart

A bot OOM-stopped and nothing relaunched it.

1. `python scripts/ops/pbtb_ops.py lambda-logs task-state --since 2h --pattern "?ERROR ?Failed ?panic"`.
2. `Failed to load configs` / missing field → **env/binary skew** (see the lambda
   row in the component map). Mitigate by restoring the key the old binary needs
   (`update-function-configuration` with the FULL variable map), then deploy the
   right binary, then `terraform apply -target=module.lambda_task_state_change_handler`
   to drop the key.
3. `SkippedNotEnabled` → the user turned the bot off; correct behaviour.
4. `SkippedSuperseded` → duplicate STOPPED event; correct behaviour.
5. Nothing logged at all → check the EventBridge rule still targets the function
   and the function has the ECS `RunTask` + `iam:PassRole` policy.
6. `python scripts/ops/pbtb_ops.py smoke-lambda task-state` must return 200.

## ci-red

1. `gh run view <run-id> -R iengai/pbtb-rust --log | grep -E "##\[error\]|error:|E: |404" | tail`.
2. A Debian `404 Not Found` on a `.deb` from `deb.debian.org`: `curl -sI <that URL>`
   from THIS machine. If it is 200 here, the runner's CDN edge has a stale
   negative cache — retrying rarely helps; remove the dependency on that package
   instead (the builder needs only `ca-certificates`).
3. `Could not assume role with OIDC` → the role trust `sub` does not match: jobs
   with `environment:` present `repo:<repo>:environment:<name>`, others
   `repo:<repo>:ref:refs/heads/<branch>`. Fix the trust in terraform, apply the
   role only (`-target=aws_iam_role.<name>`).
4. `workflow … not found on the default branch` → `workflow_dispatch` needs the
   file on main; merge first.
5. CodeBuild: `python scripts/ops/pbtb_ops.py codebuild-log <build-id>`; look at the
   FAILED phase message first, then the tail. `gcc is not installed` in the
   runtime stage → a dependency without an aarch64 wheel; wheel-build it in stage 1.

## site-stale

1. `gh run list -R iengai/pbtb-rust --workflow=pages-publish.yml --limit 3`.
2. If the last run failed at OIDC → publish role trust must be
   `environment:github-pages`.
3. If runs are green but data is old → the collector: `lambda-logs daily-pnl --since 1d`
   and `aws s3 ls s3://scalable-cluster-dev-return-charts/charts/` timestamps.
4. `gh api repos/iengai/pbtb-rust/pages --jq .build_type` must be `workflow`.

## engine-routing

1. `bot-status <bot_id>` — compare `cfg-engine` (what the config targets) with the
   task-def family the task runs on. A mismatch is normal between choosing a config
   and the next Stop/Run; it is a bug only if a *fresh* launch landed on the wrong
   family.
2. `deploy-audit` — the engine table in the lambda and on the telebot host must
   both match the latest revisions; a stale table launches an old revision.
3. Confirm the config itself: `aws s3 cp s3://scalable-cluster-dev-bot-configs/<uid>/<bot>/<bot>.json - | head -c 400`
   → `config_version`. No stamp is legal (legacy) and routes by shape.
