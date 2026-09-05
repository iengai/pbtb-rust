---
name: pbtb-triage
description: Autonomous debugging for this trading system (telebot, passivbot ECS bots, the restart and collector lambdas, CI builds, the return-curve site). Use this the moment anything in the deployment misbehaves or the user reports a problem — a Telegram error with a "ref:" id, a bot that is not running or keeps restarting, a failed GitHub Actions / CodeBuild run, a lambda that stopped restarting bots, a stale website — even if they only paste a screenshot, a log line, or say "看一下 / 查一下 / 报错了". Use it BEFORE proposing any fix; it encodes where every component lives and the root causes that have already bitten this project.
---

# pbtb triage

Every incident so far in this project had the same shape: the code was fine,
the cost was **re-discovering where things live** and **finding the drift**
between what is deployed and what the repo/terraform say. This skill front-loads
both so you spend your time on the actual cause.

## The loop

1. **Locate the component** — read [references/component-map.md](references/component-map.md)
   for the one involved. It says where it runs, where its logs are, how to reach
   them, and which quirks will otherwise cost you 20 minutes (telebot is NOT in
   ECS; the ECS host is NOT SSM-managed; Git Bash mangles `/aws/...` paths).
2. **Get the primary evidence** — the line with the real cause, not the symptom.
   A Telegram "ref: xxxxxxxx" is a redaction id; the matching `journalctl` line
   on the NAT host holds the unredacted error. Use the scripts, not ad-hoc CLI:
   ```
   python scripts/ops/pbtb_ops.py telebot-logs --grep <ref>
   python scripts/ops/pbtb_ops.py lambda-logs task-state --pattern "?ERROR ?panic"
   python scripts/ops/pbtb_ops.py codebuild-log <build-id>
   ```
3. **Check for drift before theorising** — run
   `python scripts/ops/pbtb_ops.py deploy-audit` and
   `python scripts/ops/pbtb_ops.py bot-status all --memory`.
   Read the `findings` / `ATTENTION` sections. Four of the last five root causes
   were drift: a two-month-old telebot binary reading rows it did not understand,
   a lambda whose env changed under an old binary, a CDN edge caching a 404, a
   new upstream dependency the image had no compiler for.
4. **Confirm the hypothesis with a direct probe**, never by inference:
   `curl` the failing URL from *this* machine, `smoke-lambda`, read the exact
   env key, describe the exact task definition. "Probably" is not a finding.
5. **If there is live impact, mitigate first, then root-fix, then remove the
   mitigation** — and verify after each of the three steps. Example that
   worked: put the old env key back so the old lambda binary loads (minutes),
   fix the build (an hour), then let `terraform apply` drop the key again.
   Do not let a mitigation silently become the fix.
6. **Record what was non-obvious** in memory and, if it changes how a component
   is operated, in the component map or RUNBOOK — in the same session.

## Symptom router

| Symptom | Start here |
|---|---|
| Telegram shows `❌ … (ref: xxxxxxxx)` | [playbook: telegram-ref](references/symptom-playbooks.md#telegram-ref) |
| A bot is not running / keeps restarting / "Run" does nothing | [playbook: bot-not-running](references/symptom-playbooks.md#bot-not-running) |
| A bot stopped (OOM) and was not restarted | [playbook: no-auto-restart](references/symptom-playbooks.md#no-auto-restart) |
| GitHub Actions or CodeBuild is red | [playbook: ci-red](references/symptom-playbooks.md#ci-red) |
| The return-curve site is stale or shows wrong data | [playbook: site-stale](references/symptom-playbooks.md#site-stale) |
| A bot launched on the wrong passivbot engine, or was refused | [playbook: engine-routing](references/symptom-playbooks.md#engine-routing) |
| "Everything looks fine but I want to be sure" | `deploy-audit` + `bot-status all --memory`, then stop |

## Judgement rules

- **A finding must be reproducible from a command in the report.** Show the
  command and the line it produced; the user should be able to re-run it.
- **Distinguish the trigger from the root cause.** "I wrote 24 new rows" was the
  trigger; "the running binary predates the code that skips those rows" was the
  cause; the fix was deploying, not deleting rows. Say which is which.
- **Memory is a first-class signal.** A task on an older, roomier task-def
  revision that already uses more than the latest revision's limit will OOM on
  its next restart — `bot-status --memory` flags exactly this. Treat it as an
  incident waiting to happen, not trivia.
- **Read-only until you have the cause.** Every script here is read-only except
  `smoke-lambda`, whose event the handler discards by design. Do not restart,
  redeploy, or delete anything as a diagnostic step.
- **Two failures with the same message are still two hypotheses.** The lambda
  build 404 looked like "Debian EOL"; a curl from another network showed the file
  served fine — the fix was removing an unneeded package, not switching mirrors.

## Report format

```
## 症状
## 根因（trigger vs cause）
## 证据（命令 + 输出行）
## 影响范围
## 已做的止血 / 修复 / 待撤的止血
## 验证
## 记录（memory / RUNBOOK 更新）
```
