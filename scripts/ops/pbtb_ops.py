#!/usr/bin/env python3
"""Read-only operations CLI for the pbtb-rust dev deployment.

One file, several subcommands, so every skill and every human can get the
same answers with one command instead of re-deriving where things live:

  bot-status [BOT_ID|all] [--memory]   desired vs observed state per bot
  deploy-audit                         what is deployed vs what main/terraform say (and the
                                       sha telebot runs: the rollback point)
  rollback-plan PR [--running-sha SHA] how to revert a merged PR and redeploy, or why an agent
                                       may not do it alone (exit 2)
  telebot-logs [--since 30m] [--grep X] [--app] telebot logs on the NAT host (journald, or the app with --app)
  lambda-logs NAME [--since 30m] [--pattern X]
  codebuild-log BUILD_ID [--tail N]    tolerant of the corrupt JSON CloudWatch emits
  smoke-lambda NAME                    invoke with an event the handler ignores (mcp-http: an
                                       unauthenticated HTTP request, expected to be refused 401)
  user-show USER_ID                    the account row and the identities it holds
  user-create USER_ID [--vip N] [--email E] [--telegram TG_ID]
                                       create an account row (and bind a Telegram id)
  set-vip USER_ID LEVEL                change an existing account's level
  set-role USER_ID operator|member     the operator's account may put its bots on the showcase page
  set-public-url USER_ID BOT_ID URL|off
                                       the bot's Bybit page for the showcase (the /public write,
                                       without its operator check; `off` lowercase)
  user-status USER_ID active|suspended

Everything here is read-only except `smoke-lambda`, which invokes a function
with an event its guard clause discards before any side effect, and the
`user-*` / `set-vip` / `set-role` / `set-public-url` commands, which write account
and bot rows to the bots table (each is a conditional write: create refuses an
existing account, the updates refuse a missing row).

Design notes (the reasons this is Python and not bash):
  - Windows Git Bash mangles `/aws/...` paths and collapses backslashes in
    heredocs; subprocess -> aws.exe has neither problem.
  - CloudWatch `get-log-events --output json` can contain bytes that break
    JSON parsers; codebuild-log decodes with errors="replace" and salvages.
  - The ECS container host is NOT SSM-managed, so memory comes from Container
    Insights, not `docker stats`. The NAT host IS SSM-managed (telebot lives
    there as a systemd docker service, not in ECS).
"""
from __future__ import annotations

import argparse
import json
import os
import re
import shlex
import shutil
import subprocess
import sys
import tempfile
import time
import unicodedata
from datetime import datetime, timedelta, timezone

# Everything deployment-specific is overridable by env so the same CLI serves a
# fork of this setup: PBTB_PROJECT, PBTB_ENV, PBTB_REGION, PBTB_AWS_PROFILE,
# PBTB_NAT_TAG (Name tag of the NAT/telebot host).
PROJECT = os.environ.get("PBTB_PROJECT", "scalable-cluster")
DEFAULT_ENV = os.environ.get("PBTB_ENV", "dev")
DEFAULT_REGION = os.environ.get("PBTB_REGION", "ap-northeast-1")
DEFAULT_PROFILE = os.environ.get("PBTB_AWS_PROFILE", "dev")
NAT_TAG = os.environ.get("PBTB_NAT_TAG", "nat-instance")

# ---------------------------------------------------------------- plumbing


def cfg(env: str) -> dict:
    p = f"{PROJECT}-{env}"
    return {
        "env": env,
        "cluster": f"{p}-cluster",
        "table": f"{p}-bots",
        "config_bucket": f"{p}-bot-configs",
        "chart_bucket": f"{p}-return-charts",
        "passivbot_family_prefix": f"{p}-passivbot",
        "telebot_repo": f"{p}-telebot",
        "base_env_param": f"/{PROJECT}/{env}/telebot/base-env",
        "nat_tag_name": NAT_TAG,
        "lambdas": {
            "task-state": f"{p}-task-state-change-handler",
            "daily-pnl": f"{p}-daily-pnl-snapshot",
            "hl-candles": f"{p}-hl-candle-collector",
            # Created only while `mcp_http_enabled` is set; in an env where it is
            # off the name resolves to a function that does not exist.
            "mcp-http": f"{p}-mcp-http",
        },
        "ci_log_group": f"/aws/ecs/containerinsights/{p}-cluster/performance",
    }


AWS = shutil.which("aws") or "aws"


def aws(args: list[str], profile: str, region: str, *, raw: bool = False):
    """Run the AWS CLI and return parsed JSON (or bytes when raw=True)."""
    # PYTHONUTF8: the AWS CLI is itself Python; on this cp932 console it dies with
    # UnicodeEncodeError halfway through emitting a log that contains box-drawing
    # characters, which is what "corrupt JSON from get-log-events" really was.
    env = dict(os.environ, AWS_DEFAULT_REGION=region,
               AWS_PAGER="", MSYS_NO_PATHCONV="1", PYTHONUTF8="1", PYTHONIOENCODING="utf-8")
    # An empty profile (PBTB_AWS_PROFILE="") means "whatever the environment
    # holds": in CI the OIDC step exports keys and no named profile exists.
    if profile:
        env["AWS_PROFILE"] = profile
    cmd = [AWS, *args]
    if not raw:
        cmd += ["--output", "json"]
    r = subprocess.run(cmd, capture_output=True, env=env)
    if r.returncode != 0:
        err = r.stderr.decode("utf-8", "replace").strip()
        raise RuntimeError(f"aws {' '.join(args[:3])} failed: {err.splitlines()[-1] if err else r.returncode}")
    if raw:
        return r.stdout
    out = r.stdout.decode("utf-8", "replace")
    return json.loads(out) if out.strip() else None


def dyn_val(item: dict, key: str):
    v = item.get(key)
    if not v:
        return None
    return v.get("S") or v.get("N") or v.get("BOOL")


def utc(iso_or_docker: str) -> str:
    """Render an ISO-8601 time (any offset, or docker's nanosecond form) as UTC."""
    try:
        t = re.sub(r"\.\d+", "", iso_or_docker.strip()).replace("Z", "+00:00")
        return datetime.fromisoformat(t).astimezone(timezone.utc).strftime("%Y-%m-%d %H:%MZ")
    except (TypeError, ValueError):
        return iso_or_docker[:19]


def ts(epoch_s) -> str:
    try:
        return datetime.fromtimestamp(int(epoch_s), tz=timezone.utc).strftime("%Y-%m-%d %H:%MZ")
    except (TypeError, ValueError):
        return "-"


def since_to_ms(s: str) -> int:
    m = re.fullmatch(r"(\d+)([smhd])", s.strip())
    if not m:
        raise SystemExit(f"--since must look like 30m / 2h / 1d, got {s!r}")
    n, unit = int(m.group(1)), m.group(2)
    delta = {"s": 1, "m": 60, "h": 3600, "d": 86400}[unit] * n
    return int((time.time() - delta) * 1000)


def engine_line(config: dict):
    """Mirror of domain::engine::EngineVersion::of_config."""
    cv = config.get("config_version")
    if isinstance(cv, str):
        head = cv.strip().lstrip("vV").split(".")[0]
        return int(head) if head.isdigit() else None
    if cv is not None:
        return None
    bot = config.get("bot") or {}
    for side in ("long", "short"):
        if isinstance((bot.get(side) or {}).get("risk"), dict):
            return 8
    return 7


def family_engine(family: str, prefix: str):
    """The engine-table key a family serves.

    '…-passivbot' -> '7' (the line that inherited the unsuffixed family),
    '…-passivbot-v8' -> '8', '…-passivbot-v8-rs' -> '8rs'.

    Returned as the key string rather than a number because that string is the
    contract: it is what var.passivbot_engines is keyed by and what the telebot
    and the lambda parse out of APP__ECS__TD_PASSIVBOT_BY_ENGINE. A family whose
    key does not round-trip is precisely the drift this audit exists to catch.
    """
    suffix = family[len(prefix):]
    if suffix == "":
        return "7"
    m = re.fullmatch(r"-v(\d+)(-rs)?", suffix)
    return f"{m.group(1)}{'rs' if m.group(2) else ''}" if m else None


def is_shadow_family(family: str, prefix: str) -> bool:
    """A dry-run pb-runner shadow: '…-passivbot-v8-rs-shadow-<key>'.

    Shadows are deliberately not engine lines (terraform/envs/dev/passivbot-shadow.tf):
    they are launched by family name, never through the engine table, so a shadow
    family with no table key is expected rather than drift.
    """
    return re.fullmatch(r"-v\d+-rs-shadow-.+", family[len(prefix):]) is not None


def engine_key_line(key) -> int | None:
    """The passivbot line a key runs, dropping the runtime half: '8rs' -> 8."""
    m = re.fullmatch(r"(\d+)(rs)?", str(key or ""))
    return int(m.group(1)) if m else None


def table(rows: list[list[str]], header: list[str]) -> str:
    widths = [max(len(str(x)) for x in col) for col in zip(header, *rows)] if rows else [len(h) for h in header]
    fmt = "  ".join("{:<%d}" % w for w in widths)
    out = [fmt.format(*header), fmt.format(*["-" * w for w in widths])]
    out += [fmt.format(*[str(x) for x in r]) for r in rows]
    return "\n".join(out)


# ---------------------------------------------------------------- bot-status


def cmd_bot_status(a):
    c = cfg(a.env)
    # Exactly the attributes this command reads, and never api_key /
    # secret_key: the incident-diagnose CI role allows a scan only with this
    # projection (terraform/envs/dev/diagnose-ci.tf), so widening it here
    # widens IAM too. `name` and `status` are DynamoDB reserved words.
    items = aws(["dynamodb", "scan", "--table-name", c["table"],
                 "--projection-expression", "pk, sk, #n, enabled, applied_at, #s, task_id",
                 "--expression-attribute-names", '{"#n": "name", "#s": "status"}'],
                a.profile, a.region)["Items"]
    bots, runtimes, switches = {}, {}, {}
    for it in items:
        pk, sk = dyn_val(it, "pk") or "", dyn_val(it, "sk") or ""
        # Account, identity and ticket rows live under their own partition
        # prefixes; inside a tenant partition every non-bot row carries a
        # `<kind>#` sort key (docs/data-model.md, "Row shapes and the readers").
        if not pk.startswith("user_id#"):
            continue
        uid = pk.split("#", 1)[1]
        if sk.startswith("ecs_task_metadata#"):
            runtimes[sk.split("#", 1)[1]] = it
        elif sk.startswith("config_switch#"):
            bid = sk.split("#")[1]
            prev = switches.get(bid)
            if not prev or int(dyn_val(it, "applied_at") or 0) > int(dyn_val(prev, "applied_at") or 0):
                switches[bid] = it
        elif "#" in sk:
            continue
        else:
            bots[sk] = (uid, it)

    wanted = [b for b in bots if a.bot in ("all", b)]
    if not wanted:
        raise SystemExit(f"no bot {a.bot!r} in {c['table']} (known: {', '.join(sorted(bots))})")

    # Running tasks -> keyed by the BOT_ID container override.
    arns = aws(["ecs", "list-tasks", "--cluster", c["cluster"], "--desired-status", "RUNNING"],
               a.profile, a.region)["taskArns"]
    tasks = {}
    if arns:
        for t in aws(["ecs", "describe-tasks", "--cluster", c["cluster"], "--tasks", *arns],
                     a.profile, a.region)["tasks"]:
            env = ((t.get("overrides") or {}).get("containerOverrides") or [{}])[0].get("environment") or []
            bid = next((e["value"] for e in env if e.get("name") == "BOT_ID"), None)
            if bid:
                tasks[bid] = t

    # Hard memory limit of each family's LATEST revision: a task still on an older,
    # roomier revision inherits this limit on its next restart.
    latest_limit = {}
    if a.memory:
        fams = aws(["ecs", "list-task-definition-families", "--family-prefix", c["passivbot_family_prefix"],
                    "--status", "ACTIVE"], a.profile, a.region)["families"]
        for fam in fams:
            td = aws(["ecs", "describe-task-definition", "--task-definition", fam], a.profile, a.region)["taskDefinition"]
            latest_limit[fam] = (int(td["memory"]) if td.get("memory") else None, td["revision"])

    rows, warnings = [], []
    for bid in sorted(wanted):
        uid, it = bots[bid]
        name = dyn_val(it, "name") or "?"
        enabled = dyn_val(it, "enabled")
        rt = runtimes.get(bid)
        rt_status = dyn_val(rt, "status") if rt else "-"
        rt_task = (dyn_val(rt, "task_id") or "-")[:8] if rt else "-"

        t = tasks.get(bid)
        if t:
            fam_rev = t["taskDefinitionArn"].split("/")[-1]
            family, rev = fam_rev.rsplit(":", 1)
            image_tag = (t["containers"][0].get("image") or "").split(":")[-1]
            started = (t.get("startedAt") or "")[:16]
            running_engine = family_engine(family, c["passivbot_family_prefix"])
        else:
            fam_rev, image_tag, started, running_engine = "-", "-", "-", None

        # Stored config -> the engine line the NEXT launch will use.
        key = f"{uid}/{bid}/{bid}.json"
        try:
            body = aws(["s3", "cp", f"s3://{c['config_bucket']}/{key}", "-"], a.profile, a.region, raw=True)
            config = json.loads(body.decode("utf-8", "replace"))
            cv = config.get("config_version") or "(none)"
            tpl = (config.get("pbtb") or {}).get("name") or config.get("strategy_name") or "-"
            cfg_engine = engine_line(config)
        except Exception as e:  # noqa: BLE001 - a missing config is a finding, not a crash
            cv, tpl, cfg_engine = f"ERR {e}", "-", None

        sw = switches.get(bid)
        last_switch = f"{ts(dyn_val(sw, 'applied_at'))} -> {dyn_val(sw, 'template_name')}" if sw else "-"

        # Only the line is comparable: the `rs` half of the key comes from the
        # bot's own runtime attribute, which the config does not select.
        running_line = engine_key_line(running_engine)
        if t and cfg_engine and running_line and cfg_engine != running_line:
            warnings.append(f"{name} ({bid}): config targets v{cfg_engine} but the running task is on v{running_engine} "
                            f"-> takes effect on the next Run, Restart or auto-restart")
        if enabled and not t and rt_status != "stopped":
            warnings.append(f"{name} ({bid}): enabled=true, runtime={rt_status}, but no RUNNING task")
        if not enabled and t:
            warnings.append(f"{name} ({bid}): enabled=false but a task is RUNNING (stop in flight?)")
        if cfg_engine is None:
            warnings.append(f"{name} ({bid}): config_version {cv!r} is unparseable -> Run will be refused")

        mem = ""
        if a.memory and t:
            util, res = task_memory(c, t["taskArn"].split("/")[-1], a)
            mem = f"{util}/{res}" if util is not None else "(no sample)"
            if util is not None and res:
                if util / res >= 0.85:
                    warnings.append(f"{name} ({bid}): using {util} of its {res} MB limit ({util * 100 // res}%)")
                lim, latest_rev = latest_limit.get(family, (None, None))
                if lim and int(rev) != latest_rev and util > lim:
                    warnings.append(f"{name} ({bid}): uses {util} MB but {family}:{latest_rev} caps at {lim} MB -> "
                                    f"it would OOM immediately on its next restart; raise that line's memory "
                                    f"(passivbot_engines[..].memory) BEFORE it restarts")

        rows.append([bid, name, str(enabled), rt_status, rt_task, fam_rev, image_tag, started,
                     f"v{cfg_engine}" if cfg_engine else "?", cv, tpl, last_switch] + ([mem] if a.memory else []))

    header = ["bot_id", "name", "enabled", "runtime", "task", "task-def", "image", "started",
              "cfg-engine", "config_version", "template", "last switch"] + (["mem MB (util/res)"] if a.memory else [])
    print(table(rows, header))
    if warnings:
        print("\nATTENTION")
        for w in warnings:
            print(" -", w)


def task_memory(c: dict, task_id: str, a):
    """Latest Container Insights sample for one task, as (utilized_mb, reserved_mb).
    The ECS host is not SSM-managed, so this is the only per-task memory source."""
    try:
        ev = aws(["logs", "filter-log-events", "--log-group-name", c["ci_log_group"],
                  "--start-time", str(since_to_ms("20m")),
                  "--filter-pattern", f'{{ $.Type = "Task" && $.TaskId = "{task_id}" }}',
                  "--query", "events[-1].message"], a.profile, a.region)
        if not ev:
            return None, None
        d = json.loads(ev)
        return int(d.get("MemoryUtilized") or 0), int(d.get("MemoryReserved") or 0)
    except Exception:  # noqa: BLE001
        return None, None


# ---------------------------------------------------------------- deploy-audit


def git(args: list[str]) -> str:
    r = subprocess.run(["git", *args], capture_output=True, text=True)
    return r.stdout.strip() if r.returncode == 0 else ""


def cmd_deploy_audit(a):
    c = cfg(a.env)
    findings = []

    # --- main
    main_sha = git(["ls-remote", "origin", "main"]).split()[0][:40] if git(["ls-remote", "origin", "main"]) else ""
    print(f"main (origin): {main_sha[:7] or '?'}")

    # --- telebot: ECR latest vs what the NAT host runs
    print("\n== telebot ==")
    imgs = aws(["ecr", "describe-images", "--repository-name", c["telebot_repo"],
                "--query", "reverse(sort_by(imageDetails,&imagePushedAt))[:1]"], a.profile, a.region)
    latest = imgs[0] if imgs else {}
    latest_tags = latest.get("imageTags") or []
    latest_sha = next((t for t in latest_tags if re.fullmatch(r"[0-9a-f]{40}", t)), None)
    print(f"ECR :latest -> {latest_sha[:7] if latest_sha else '?'} pushed {latest.get('imagePushedAt', '?')[:16]}")
    if main_sha and latest_sha and latest_sha != main_sha:
        git(["fetch", "origin", "main", "-q"])
        behind = git(["rev-list", "--count", f"{latest_sha}..origin/main"]) or "?"
        findings.append(f"telebot image :latest is {behind} commit(s) behind main "
                        f"(telebot-build only runs on src/Cargo/Dockerfile changes; may be expected)")
    nat = nat_instance(c, a)
    host_probe_err, host_probed = None, False
    if nat:
        # Labelled lines: robust to blank lines and to any one command failing.
        try:
            out = ssm(nat, [
                "echo CREATED=$(docker inspect --format '{{.Created}}' telebot 2>/dev/null)",
                "echo DIGEST=$(docker image inspect --format '{{index .RepoDigests 0}}' $(docker inspect --format '{{.Image}}' telebot 2>/dev/null) 2>/dev/null)",
                "echo TABLE=$(grep -E '^APP__ECS__TD_PASSIVBOT_BY_ENGINE=' /etc/telebot/telebot.env 2>/dev/null | cut -d= -f2-)",
            ], a)
            host_probed = True
        except RuntimeError as e:
            # A principal without ssm:SendCommand (the diagnose CI role) still
            # gets every other section; the host is reported as unprobed, not down.
            host_probe_err, out = str(e), ""
        kv = dict(l.split("=", 1) for l in (out or "").splitlines() if "=" in l)
        created, repo_digest, host_table = kv.get("CREATED", ""), kv.get("DIGEST", ""), kv.get("TABLE") or None
        host_digest = repo_digest.split("@")[-1] if "@" in repo_digest else None
        if host_probe_err:
            print(f"NAT host container: not probed ({host_probe_err})")
            findings.append("telebot host not probed from here (no SSM access); check telebot via Sentry or from a workstation")
        elif not created:
            print("NAT host container: NOT RUNNING")
            findings.append("telebot container is not running on the NAT host")
        else:
            print(f"NAT host container: started {utc(created)}  digest {(host_digest or '?')[7:19]}")
            if host_digest:
                running = aws(["ecr", "describe-images", "--repository-name", c["telebot_repo"],
                               "--image-ids", f"imageDigest={host_digest}",
                               "--query", "imageDetails[0].imageTags"], a.profile, a.region) or []
                running_sha = next((t for t in running if re.fullmatch(r"[0-9a-f]{40}", t)), None)
                print(f"telebot running sha: {running_sha or '?'}  (record it before a telebot-deploy: "
                      f"the rollback point, pbtb-deploy § Rolling back)")
            # Digest is the truth: a rebuild that changes no bytes (docs-only commit) still
            # re-tags :latest, and timestamps alone would call that "stale".
            if host_digest and latest.get("imageDigest") and host_digest != latest["imageDigest"]:
                findings.append(f"telebot on the NAT host runs digest {host_digest[7:19]} but ECR :latest is "
                                f"{latest['imageDigest'][7:19]} (pushed {utc(latest['imagePushedAt'])}) "
                                f"-> stale binary; run telebot-deploy")
            elif host_digest is None:
                findings.append("could not read the telebot image digest on the NAT host (docker inspect RepoDigests empty)")
        if host_probed and not host_table:
            findings.append("telebot.env on the NAT host has no APP__ECS__TD_PASSIVBOT_BY_ENGINE (pre-#34 env)")
    else:
        findings.append("NAT instance (tag Name=nat-instance) not found / not running")
        host_table = None

    # --- passivbot task definitions per engine line
    print("\n== passivbot task definitions ==")
    fams = aws(["ecs", "list-task-definition-families", "--family-prefix", c["passivbot_family_prefix"],
                "--status", "ACTIVE"], a.profile, a.region)["families"]
    latest_by_family = {}
    for fam in sorted(fams):
        td = aws(["ecs", "describe-task-definition", "--task-definition", fam], a.profile, a.region)["taskDefinition"]
        latest_by_family[fam] = td
        eng = family_engine(fam, c["passivbot_family_prefix"])
        shadow = is_shadow_family(fam, c["passivbot_family_prefix"])
        print(f"{eng or ('shadow' if shadow else '?')}: {fam}:{td['revision']}  image={td['containerDefinitions'][0]['image'].split(':')[-1]}  memory={td.get('memory')}")
        if eng is None and not shadow:
            findings.append(f"family {fam} maps to no engine-table key -> nothing can launch on it "
                            f"(the family suffix and var.passivbot_engines key have diverged)")
    running = aws(["ecs", "list-tasks", "--cluster", c["cluster"], "--desired-status", "RUNNING"], a.profile, a.region)["taskArns"]
    if running:
        for t in aws(["ecs", "describe-tasks", "--cluster", c["cluster"], "--tasks", *running], a.profile, a.region)["tasks"]:
            fam_rev = t["taskDefinitionArn"].split("/")[-1]
            fam, rev = fam_rev.rsplit(":", 1)
            cur = latest_by_family.get(fam)
            if cur and int(rev) != cur["revision"]:
                findings.append(f"task {fam_rev} ({t['taskArn'].split('/')[-1][:8]}) runs an older revision than "
                                f"{fam}:{cur['revision']} (picks up the new one on its next restart)")

    # --- the engine table each launcher holds vs the latest revisions
    expected = {key: td["taskDefinitionArn"] for f, td in latest_by_family.items()
                if (key := family_engine(f, c["passivbot_family_prefix"]))}
    print("\n== engine table: lambda vs telebot vs latest revisions ==")
    fn = c["lambdas"]["task-state"]
    lam = aws(["lambda", "get-function-configuration", "--function-name", fn], a.profile, a.region)
    lam_env = (lam.get("Environment") or {}).get("Variables") or {}
    lam_table = lam_env.get("APP__ECS__TD_PASSIVBOT_BY_ENGINE")
    print(f"lambda {fn}: sha={lam['CodeSha256'][:12]} modified={lam['LastModified'][:16]}")
    # The host's table is only comparable when the host answered; an unprobed
    # or absent host has its own finding above and must not read as "no table".
    for label, tbl in [("lambda", lam_table)] + ([("telebot", host_table)] if host_probed else []):
        if not tbl:
            findings.append(f"{label} has no engine table")
            continue
        got = dict(e.split("=", 1) for e in tbl.split(",") if "=" in e)
        for eng, arn in expected.items():
            if got.get(eng) != arn:
                findings.append(f"{label} engine {eng} -> {got.get(eng, 'MISSING').split('/')[-1]} "
                                f"but latest is {arn.split('/')[-1]} -> re-run "
                                f"{'terraform apply (lambda target)' if label == 'lambda' else 'telebot-deploy'}")
        for eng in got:
            if eng not in expected:
                findings.append(f"{label} registers engine {eng} which has no task-definition family")
    for k in ("APP__ECS__TD_PASSIVBOT_ARN",):
        if k in lam_env:
            findings.append(f"lambda still carries {k} (pre-#34 key / leftover mitigation) -> apply lambda target")

    # --- other lambdas: just show
    for short, fname in c["lambdas"].items():
        if fname == fn:
            continue
        l2 = aws(["lambda", "get-function-configuration", "--function-name", fname], a.profile, a.region)
        print(f"lambda {fname}: sha={l2['CodeSha256'][:12]} modified={l2['LastModified'][:16]}")

    # --- the image each line launches from
    #
    # A line names its own repo -- the rs line runs the pb-runner image, not
    # passivbot-live -- so each tag is looked up in the repo its own image URI
    # names. Checking one hard-coded repo reported a tag that was present as
    # missing, which reads exactly like a broken line.
    by_repo = {}
    for fam, td in latest_by_family.items():
        image = td["containerDefinitions"][0]["image"]
        repo, _, tag = image.split("/", 1)[-1].rpartition(":")
        by_repo.setdefault(repo, []).append((fam, tag))
    for repo in sorted(by_repo):
        print(f"\n== {repo} images ==")
        tags = aws(["ecr", "describe-images", "--repository-name", repo,
                    "--query", "sort_by(imageDetails,&imagePushedAt)[?imageTags].[imagePushedAt,imageTags]"],
                   a.profile, a.region) or []
        for pushed, itags in tags:
            print(f"  {pushed[:10]}  {', '.join(itags)}")
        for fam, tag in by_repo[repo]:
            if not any(tag in itags for _, itags in tags):
                findings.append(f"{fam} points at {repo}:{tag} which is NOT in ECR -> a launch on this line will fail at pull")

    print("\n== findings ==")
    if findings:
        for f in findings:
            print(" -", f)
    else:
        print(" (none) deployed state matches main / terraform / ECR")


# ---------------------------------------------------------------- SSM helpers


def nat_instance(c: dict, a) -> str | None:
    r = aws(["ec2", "describe-instances", "--filters", f"Name=tag:Name,Values={c['nat_tag_name']}",
             "Name=instance-state-name,Values=running",
             "--query", "Reservations[].Instances[].InstanceId"], a.profile, a.region)
    return r[0] if r else None


def ssm_wait_online(instance: str, a, timeout_s: int = 300) -> None:
    """Block until SSM will accept a command for this instance.

    An instance that has just booted is not yet a valid SendCommand target:
    the agent registers seconds to a couple of minutes after the API reports
    the instance running, and until it does SendCommand fails outright with
    `InvalidInstanceId: Instances not in a valid state for account`. Anything
    that talks to a freshly created instance has to wait for this first --
    the NAT rebuild's step 2 runs against a standby step 1 created moments
    earlier, so without the wait it aborts the window every time.
    """
    deadline = time.time() + timeout_s
    while True:
        info = aws(["ssm", "describe-instance-information",
                    "--filters", f"Key=InstanceIds,Values={instance}",
                    "--query", "InstanceInformationList[0].PingStatus"], a.profile, a.region)
        if info == "Online":
            return
        if time.time() >= deadline:
            raise RuntimeError(
                f"{instance} never came Online in SSM within {timeout_s}s "
                f"(last PingStatus: {info or 'not registered'})")
        time.sleep(5)


def ssm(instance: str, commands: list[str], a, timeout_s: int = 60) -> str:
    ssm_wait_online(instance, a)
    params = json.dumps({"commands": commands})
    cid = aws(["ssm", "send-command", "--instance-ids", instance, "--document-name", "AWS-RunShellScript",
               "--parameters", params, "--query", "Command.CommandId"], a.profile, a.region)
    for _ in range(timeout_s // 3):
        time.sleep(3)
        inv = aws(["ssm", "get-command-invocation", "--command-id", cid, "--instance-id", instance], a.profile, a.region)
        if inv["Status"] in ("Success", "Failed", "Cancelled", "TimedOut"):
            out = inv.get("StandardOutputContent", "")
            if inv["Status"] != "Success":
                out += "\n[stderr] " + inv.get("StandardErrorContent", "")
            return out.rstrip()
    return "(ssm timed out)"


def cmd_telebot_logs(a):
    c = cfg(a.env)
    nat = nat_instance(c, a)
    if not nat:
        raise SystemExit("NAT instance not found")
    mins = max(1, (int(time.time() * 1000) - since_to_ms(a.since)) // 60000)
    cmds = []
    if a.env_dump:
        cmds.append("echo '== /etc/telebot/telebot.env (non-secret keys) =='; grep -E '^(APP__|PBTB_)' /etc/telebot/telebot.env")
    cmds.append("echo '== container =='; docker ps --filter name=telebot --format '{{.Image}} | {{.Status}}'; "
                "echo restarts=$(docker inspect --format '{{.RestartCount}}' telebot 2>/dev/null)")
    # journald holds only the wrapper's output (docker login, image pull,
    # "Starting Telegram bot..."); the application's own lines are in the
    # container's log.
    if a.app:
        source, j = "docker logs", f"docker logs telebot --since {mins}m 2>&1"
    else:
        source, j = "journal", f"journalctl -u telebot --since '{mins} minutes ago' --no-pager"
    if a.grep:
        j += f" | grep -iE -A {a.context} '{a.grep}'"
    else:
        j += f" | tail -n {a.tail}"
    cmds.append(f"echo '== {source} =='; " + j + " || true")
    print(ssm(nat, cmds, a, timeout_s=90))


def cmd_lambda_logs(a):
    c = cfg(a.env)
    fname = c["lambdas"].get(a.name, a.name)
    args = ["logs", "filter-log-events", "--log-group-name", f"/aws/lambda/{fname}",
            "--start-time", str(since_to_ms(a.since)), "--query", "events[].message"]
    if a.pattern:
        args += ["--filter-pattern", a.pattern]
    msgs = aws(args, a.profile, a.region) or []
    for m in msgs[-a.tail:]:
        print(m.rstrip()[:300])
    print(f"({len(msgs)} events in the last {a.since})")


def cmd_codebuild_log(a):
    b = aws(["codebuild", "batch-get-builds", "--ids", a.build_id], a.profile, a.region)["builds"][0]
    print(f"status={b['buildStatus']} phase={b.get('currentPhase')}")
    for ph in b.get("phases", []):
        if ph.get("phaseStatus") == "FAILED":
            print(f"FAILED phase {ph['phaseType']}: {(ph.get('contexts') or [{}])[0].get('message')}")
    g, s = b["logs"]["groupName"], b["logs"]["streamName"]
    raw = aws(["logs", "get-log-events", "--log-group-name", g, "--log-stream-name", s,
               "--limit", str(max(a.tail, 400)), "--query", "events[].message"], a.profile, a.region, raw=True)
    text = raw.decode("utf-8", "replace")
    try:
        lines = json.loads(text)
    except json.JSONDecodeError:
        # CloudWatch can emit bytes that break the CLI's own JSON; salvage strings.
        lines = [m.group(1).encode().decode("unicode_escape", "replace")
                 for m in re.finditer(r'^\s*"(.*)",?\s*$', text, re.M)]
    clean = []
    for l in lines:
        l = re.sub(r"[^\x20-\x7e]", "", l.rstrip())
        if l and not re.search(r"Downloading|Collecting|Requirement already|Using cached", l):
            clean.append(l[:200])
    # The interesting part of a failed build is what led up to the first hard error,
    # not the wrap-up phases that follow it.
    first_err = next((i for i, l in enumerate(clean)
                      if re.search(r"ERROR:|error:|exit code: [1-9]|failed to solve|Failed to build|E: ", l)), None)
    if first_err is not None:
        lo, hi = max(0, first_err - a.before), min(len(clean), first_err + a.after)
        print(f"--- {len(clean)} lines; first error at line {first_err}; showing {lo}..{hi} ---")
        for l in clean[lo:hi]:
            print(l)
    else:
        for l in clean[-a.tail:]:
            print(l)


SMOKE_EVENTBRIDGE = '{"version":"0","source":"pbtb.smoke-test","detail-type":"SmokeTest","detail":{}}'
# mcp-http is a Function URL handler: the runtime rejects an EventBridge shape
# before the handler runs, so it gets an HTTP API v2 request without a bearer.
SMOKE_HTTP_V2 = ('{"version":"2.0","rawPath":"/","requestContext":{"http":{"method":"POST","path":"/"}},'
                 '"headers":{"content-type":"application/json"},"body":"{}","isBase64Encoded":false}')


def smoke_is_http(fname: str, c: dict) -> bool:
    return fname == c["lambdas"]["mcp-http"]


def smoke_failure(is_http: bool, out: dict, body: str) -> str | None:
    """Why a smoke invoke counts as failed, or None when it passed."""
    status, ferr = out.get("StatusCode"), out.get("FunctionError")
    if status != 200:
        return f"unexpected StatusCode={status}"
    if ferr:
        return f"FunctionError={ferr}"
    if is_http:
        try:
            code = json.loads(body).get("statusCode")
        except (json.JSONDecodeError, AttributeError):
            return f"response is not an HTTP response: {body[:200]}"
        if code != 401:
            return f"an unauthenticated request returned {code}, not 401"
    return None


def cmd_smoke_lambda(a):
    c = cfg(a.env)
    fname = c["lambdas"].get(a.name, a.name)
    is_http = smoke_is_http(fname, c)
    payload = SMOKE_HTTP_V2 if is_http else SMOKE_EVENTBRIDGE
    fd, resp_path = tempfile.mkstemp(suffix=".json")
    os.close(fd)
    try:
        out = aws(["lambda", "invoke", "--function-name", fname, "--cli-binary-format", "raw-in-base64-out",
                   "--payload", payload, resp_path], a.profile, a.region)
        with open(resp_path, encoding="utf-8", errors="replace") as f:
            body = f.read()
    finally:
        os.unlink(resp_path)
    print(f"{fname}: StatusCode={out.get('StatusCode')} FunctionError={out.get('FunctionError')}")
    why = smoke_failure(is_http, out, body)
    if why:
        print(f"smoke failed: {why}")
        raise SystemExit(1)
    print("smoke ok")


# ---------------------------------------------------------------- main


# ---------------------------------------------------------------- accounts
#
# Row shapes mirror src/infra/botrepository.rs: the account row is
# pk=user#<id>/sk=profile, a Telegram identity is pk=identity#telegram#<tg>/
# sk=profile plus the tenant's own listing pk=user_id#<id>/sk=identity#telegram#<tg>.

MAX_VIP_LEVEL = 9


def dyn_put(c: dict, item: dict, condition: str, a, values: dict | None = None) -> bool:
    """put-item with a condition; False when the condition refused it."""
    env = dict(os.environ, AWS_PROFILE=a.profile, AWS_DEFAULT_REGION=a.region, AWS_PAGER="",
               MSYS_NO_PATHCONV="1", PYTHONUTF8="1", PYTHONIOENCODING="utf-8")
    cmd = [AWS, "dynamodb", "put-item", "--table-name", c["table"],
           "--item", json.dumps(item), "--condition-expression", condition]
    if values:
        cmd += ["--expression-attribute-values", json.dumps(values)]
    r = subprocess.run(cmd, capture_output=True, env=env)
    if r.returncode == 0:
        return True
    err = r.stderr.decode("utf-8", "replace")
    if "ConditionalCheckFailedException" in err:
        return False
    raise RuntimeError(f"aws dynamodb put-item failed: {err.strip().splitlines()[-1]}")


def dyn_update_user(c: dict, user_id: str, field: str, value: dict, a) -> bool:
    env = dict(os.environ, AWS_PROFILE=a.profile, AWS_DEFAULT_REGION=a.region, AWS_PAGER="",
               MSYS_NO_PATHCONV="1", PYTHONUTF8="1", PYTHONIOENCODING="utf-8")
    now = str(int(time.time()))
    r = subprocess.run([AWS, "dynamodb", "update-item", "--table-name", c["table"],
                        "--key", json.dumps({"pk": {"S": f"user#{user_id}"}, "sk": {"S": "profile"}}),
                        "--update-expression", "SET #f = :v, updated_at = :now",
                        "--condition-expression", "attribute_exists(pk)",
                        "--expression-attribute-names", json.dumps({"#f": field}),
                        "--expression-attribute-values", json.dumps({":v": value, ":now": {"N": now}})],
                       capture_output=True, env=env)
    if r.returncode == 0:
        return True
    err = r.stderr.decode("utf-8", "replace")
    if "ConditionalCheckFailedException" in err:
        return False
    raise RuntimeError(f"aws dynamodb update-item failed: {err.strip().splitlines()[-1]}")


def cmd_user_show(a):
    c = cfg(a.env)
    got = aws(["dynamodb", "get-item", "--table-name", c["table"], "--consistent-read",
               "--key", json.dumps({"pk": {"S": f"user#{a.user_id}"}, "sk": {"S": "profile"}})],
              a.profile, a.region)
    item = (got or {}).get("Item")
    if not item:
        print(f"no account row for {a.user_id}")
    else:
        print(f"user_id    {a.user_id}")
        for k in ("vip_level", "status", "role", "email", "created_at", "updated_at"):
            v = dyn_val(item, k)
            if v is not None:
                if k.endswith("_at"):
                    v = f"{v} ({ts(int(v))})"
                print(f"{k:<10} {v}")
    q = aws(["dynamodb", "query", "--table-name", c["table"],
             "--key-condition-expression", "pk = :pk AND begins_with(sk, :p)",
             "--expression-attribute-values",
             json.dumps({":pk": {"S": f"user_id#{a.user_id}"}, ":p": {"S": "identity#"}})],
            a.profile, a.region)
    ids = [dyn_val(i, "sk")[len("identity#"):] for i in (q or {}).get("Items", [])]
    print("identities " + (", ".join(ids) if ids else "(none)"))


def cmd_user_create(a):
    c = cfg(a.env)
    if not 0 <= a.vip <= MAX_VIP_LEVEL:
        raise RuntimeError(f"--vip must be within 0..{MAX_VIP_LEVEL}")
    now = str(int(time.time()))
    item = {"pk": {"S": f"user#{a.user_id}"}, "sk": {"S": "profile"},
            "vip_level": {"N": str(a.vip)}, "status": {"S": "active"},
            "created_at": {"N": now}, "updated_at": {"N": now}}
    if a.email:
        item["email"] = {"S": a.email}
    if dyn_put(c, item, "attribute_not_exists(pk)", a):
        print(f"created account {a.user_id} vip={a.vip}")
    else:
        print(f"account {a.user_id} already exists; left untouched")
    if a.telegram:
        forward = {"pk": {"S": f"identity#telegram#{a.telegram}"}, "sk": {"S": "profile"},
                   "user_id": {"S": a.user_id}, "linked_at": {"N": now}}
        # The same condition the bot uses: an identity names one tenant, and
        # re-binding to the same one is not a failure.
        if not dyn_put(c, forward, "attribute_not_exists(pk) OR user_id = :u", a,
                       values={":u": {"S": a.user_id}}):
            raise RuntimeError(f"telegram id {a.telegram} is already bound to another account")
        listing = {"pk": {"S": f"user_id#{a.user_id}"}, "sk": {"S": f"identity#telegram#{a.telegram}"},
                   "linked_at": {"N": now}}
        aws(["dynamodb", "put-item", "--table-name", c["table"], "--item", json.dumps(listing)],
            a.profile, a.region)
        print(f"bound telegram {a.telegram} -> {a.user_id}")


def cmd_set_vip(a):
    c = cfg(a.env)
    if not 0 <= a.level <= MAX_VIP_LEVEL:
        raise RuntimeError(f"level must be within 0..{MAX_VIP_LEVEL}")
    if dyn_update_user(c, a.user_id, "vip_level", {"N": str(a.level)}, a):
        print(f"{a.user_id} vip_level={a.level}")
    else:
        raise RuntimeError(f"no account row for {a.user_id} (user-create first)")


def cmd_set_role(a):
    c = cfg(a.env)
    if dyn_update_user(c, a.user_id, "role", {"S": a.role}, a):
        print(f"{a.user_id} role={a.role}")
    else:
        raise RuntimeError(f"no account row for {a.user_id} (user-create first)")


# The domain's rule for a showcase link (Bot::validate_public_url), byte for
# byte: at most 512 bytes, no whitespace or control character, https, the
# authority (up to the first of / ? # \\) a bare ASCII host on bybit.com, so
# no userinfo, no port, no IDN. The same words on refusal, so an operator
# reading either surface learns the same rule.
PUBLIC_URL_HOSTS = ("bybit.com",)


def check_public_url(url: str) -> str:
    refuse = RuntimeError("a public link must be an https URL on bybit.com")
    if len(url.encode("utf-8")) > 512 or not url.startswith("https://"):
        raise refuse
    if any(ch.isspace() or unicodedata.category(ch) == "Cc" for ch in url):
        raise refuse
    rest = url[len("https://"):]
    end = len(rest)
    for stop in "/?#\\":
        i = rest.find(stop)
        if i != -1:
            end = min(end, i)
    host = rest[:end]
    if not host or not host.isascii() or not all(ch.isalnum() or ch in ".-" for ch in host):
        raise refuse
    host = host.lower()
    if not any(host == h or host.endswith("." + h) for h in PUBLIC_URL_HOSTS):
        raise refuse
    return url


def dyn_update_bot(c: dict, user_id: str, bot_id: str, expr: str, names: dict, values: dict, a) -> bool:
    """update-item on a bot row; False when no bot row has that id.

    The condition pins the row kind, not only the partition: every row of the
    tenant lives under the same pk, and a stray `sk` would otherwise land the
    attribute on a runtime or identity row. `exchange` is a field only a bot
    row carries."""
    if "#" in bot_id:
        raise RuntimeError("a bot id carries no '#'; that is another row kind")
    env = dict(os.environ, AWS_PROFILE=a.profile, AWS_DEFAULT_REGION=a.region, AWS_PAGER="",
               MSYS_NO_PATHCONV="1", PYTHONUTF8="1", PYTHONIOENCODING="utf-8")
    now = str(int(time.time()))
    cmd = [AWS, "dynamodb", "update-item", "--table-name", c["table"],
           "--key", json.dumps({"pk": {"S": f"user_id#{user_id}"}, "sk": {"S": bot_id}}),
           "--update-expression", expr,
           "--condition-expression", "attribute_exists(pk) AND attribute_exists(exchange)",
           "--expression-attribute-values", json.dumps({**values, ":now": {"N": now}})]
    if names:
        cmd += ["--expression-attribute-names", json.dumps(names)]
    r = subprocess.run(cmd, capture_output=True, env=env)
    if r.returncode == 0:
        return True
    err = r.stderr.decode("utf-8", "replace")
    if "ConditionalCheckFailedException" in err:
        return False
    raise RuntimeError(f"aws dynamodb update-item failed: {err.strip().splitlines()[-1]}")


def cmd_set_public_url(a):
    c = cfg(a.env)
    if a.url == "off":
        done = dyn_update_bot(c, a.user_id, a.bot_id, "REMOVE public_url SET updated_at = :now", {}, {}, a)
        change = "link cleared"
    else:
        url = check_public_url(a.url)
        done = dyn_update_bot(c, a.user_id, a.bot_id, "SET public_url = :u, updated_at = :now",
                              {}, {":u": {"S": url}}, a)
        change = f"now links to {url}"
    # Only the link is reported: whether the bot is shown is its `showcase`
    # choice when it has one, which this command does not touch.
    if done:
        print(f"{a.user_id}/{a.bot_id} {change}")
    else:
        raise RuntimeError(f"no bot row {a.bot_id} under {a.user_id}")


def cmd_user_status(a):
    c = cfg(a.env)
    if dyn_update_user(c, a.user_id, "status", {"S": a.status}, a):
        print(f"{a.user_id} status={a.status}")
    else:
        raise RuntimeError(f"no account row for {a.user_id} (user-create first)")


# ---------------------------------------------------------------- rollback-plan
#
# What reverting a merged PR takes, and whether an agent may do it alone
# (docs/governance.md § Autonomy tiers; the procedure is pbtb-deploy § Rolling
# back). Read-only: it prints the commands, it runs none of them.

# A binary built past one of these and then rolled back can erase or misread
# what it stores (an older whole-row save drops attributes it does not know, an
# older reader calls a new value a CorruptRecord, the collector reads its state
# and writes it back without the fields it does not know), or fail at config
# load against the env the newer one shipped with (pbtb-deploy, the coupling rule).
STORED_SHAPE = ("src/domain.rs", "src/domain/*", "src/infra/*repository.rs", "src/infra/*store.rs",
                "src/bin/*/model.rs", "src/bin/*/s3_writer.rs", "src/bin/*/store.rs",
                "src/bin/daily_pnl_snapshot/archive.rs", "docs/data-model.md")
ENV_SHAPE = ("src/config.rs", "src/config/*", "src/main.rs", "src/bin/*/main.rs", "src/bin/*/config.rs",
             "src/bin/shared/*", "src/observability.rs")
# What telebot-build hashes: a commit touching one of these changes the image.
TELEBOT_SOURCE = ("src", "Cargo.toml", "Cargo.lock", ".devcontainer/Dockerfile", ".dockerignore")
# Binaries an agent may redeploy itself during a rollback; the rest are named
# for the owner. The restart lambda launches live tasks, mcp-http serves every
# tenant's tools, the candle collector feeds the lab.
AGENT_BINARIES = {"telebot", "daily_pnl_snapshot"}
BINARY_DEPLOY = {
    "telebot": "gh workflow run telebot-deploy.yml --ref main -f tag={sha} -f passivbot_revisions=latest",
    "daily_pnl_snapshot": "gh workflow run daily-pnl-snapshot-deploy.yml --ref main",
    "task_state_change_handler": "gh workflow run lambda-deploy.yml --ref main -f target=task-state-change-handler",
    "mcp_http": "gh workflow run lambda-deploy.yml --ref main -f target=mcp-http",
    "hl_candle_collector": "gh workflow run lambda-deploy.yml --ref main -f target=hl-candle-collector",
}
SQUASHED = re.compile(r"\(#(\d+)\)$")


def matches(path: str, patterns) -> bool:
    import fnmatch
    return any(fnmatch.fnmatch(path, p) for p in patterns)


def codeowned(path: str, owned: list[str]) -> bool:
    """`owned` holds .github/CODEOWNERS entries without the leading slash; a trailing slash is a directory.

    A wildcard entry is matched with fnmatch, whose `*` also crosses `/`: wider than GitHub's, so a refusal errs safe.
    """
    import fnmatch
    return any(path.startswith(o) if o.endswith("/") else fnmatch.fnmatch(path, o) for o in owned)


def codeowners(text: str) -> list[str]:
    return [line.split()[0].lstrip("/") for line in text.splitlines()
            if line.strip() and not line.lstrip().startswith("#")]


def ticked_invariants(body: str) -> list[str]:
    m = re.search(r"^### Invariants this change comes near\s*\n(.*?)(?=^### |\Z)", body or "", re.S | re.M)
    return re.findall(r"^- \[[xX]\] (.+)$", m.group(1), re.M) if m else []


def same_subjects(subjects: list[str], headlines: list[str]) -> bool:
    """Whether the commits on main carry the PR's headlines; GitHub cuts a long
    headline and ends it with an ellipsis, which then matches as a prefix."""
    left = list(subjects)
    for h in headlines:
        cut = h[:-1] if h.endswith("\u2026") else None
        hit = next((s for s in left if s == h or (cut is not None and s.startswith(cut))), None)
        if hit is None:
            return False
        left.remove(hit)
    return not left


def revert_range(pr: dict, history: list[dict], range_files: set[str]) -> tuple[list[str], str]:
    """The commits on main that the PR became, newest first, or [] and why not.

    `history` is main from the merge commit backwards, each {sha, parents, subject};
    `range_files` is the files changed across the commits the PR became (one for
    a squash merge, the PR's commit count otherwise).
    A squash merge is one commit whose subject ends in `(#n)`; a rebase merge is
    the PR's commits replayed, which is only trusted when the subjects and the
    file set both match the PR.
    """
    if not history or history[0]["sha"] != pr["merge_sha"]:
        return [], "main's history does not start at the PR's merge commit"
    head = history[0]
    m = SQUASHED.search(head["subject"])
    if m and int(m.group(1)) == pr["number"]:
        if len(head["parents"]) != 1:
            return [], "the merge commit has two parents"
        if range_files != set(pr["files"]):
            return [], "the squash commit changes other files than the PR did"
        return [head["sha"]], ""
    n = len(pr["headlines"])
    span = history[:n]
    if len(span) < n:
        return [], "main is shorter than the PR's commit count"
    if any(len(c["parents"]) != 1 for c in span):
        return [], "a merge commit sits inside the range"
    if not same_subjects([c["subject"] for c in span], pr["headlines"]):
        return [], "the commits ending at the merge commit are not the PR's (subjects differ)"
    if range_files != set(pr["files"]):
        return [], "the range changes other files than the PR did"
    return [c["sha"] for c in span], ""


def rollback_refusals(pr: dict, owned: list[str], invariants: list[str]) -> list[str]:
    files = pr["files"]
    found = []
    if re.match(r"(?i)revert\b", pr["title"]):
        found.append("the PR is itself a revert; a revert of a revert is a forward fix for a person")
    if any(f.startswith("terraform/") for f in files):
        found.append("it changed terraform/: never revert and apply (a reverted user_data rebuilds the NAT); fix forward")
    hit = sorted(f for f in files if codeowned(f, owned))
    if hit:
        found.append(f"it changed code-owned paths ({', '.join(hit[:3])}{', …' if len(hit) > 3 else ''}): "
                     f"the revert waits on the owner, and the App cannot push .github/workflows/")
    for name, patterns, risk in (
            ("stored shape", STORED_SHAPE, "an older binary can erase what it does not know or misread new values"),
            ("env shape", ENV_SHAPE, "an older binary can fail at config load against the env the newer one shipped with")):
        hit = sorted(f for f in files if matches(f, patterns))
        if hit:
            found.append(f"it changed the {name} ({', '.join(hit[:3])}{', …' if len(hit) > 3 else ''}): "
                         f"{risk}; the owner decides")
    if invariants:
        found.append(f"its issue ticks an invariant ({invariants[0]}); the owner decides")
    return found


def telebot_pin_problem(running_sha: str | None, pin_is_ancestor: bool, pr_shas: list[str],
                        image_commits: list[str]) -> str:
    """Why pinning telebot to `running_sha` is not this PR's rollback, or "".

    A rollback is a rollout to an older binary: every commit between the pin and
    main that changed the image goes back with it, and the coupling rule holds
    for each of them, so the pin is the PR's rollback only when the PR's own
    commits are the only ones (`image_commits`: telebot-source commits in
    running_sha..origin/main).
    """
    if not running_sha:
        return "it changes telebot: pass --running-sha, the sha deploy-audit printed before the deploy"
    if not pin_is_ancestor:
        return f"{running_sha[:7]} is not an ancestor of the PR's base: it is not a sha from before this PR"
    others = [c for c in image_commits if c not in pr_shas]
    if others:
        return (f"{len(others)} other commit(s) changed telebot's source after {running_sha[:7]} "
                f"({', '.join(c[:7] for c in others[:3])}): pinning it rolls those back too; the owner decides")
    return ""


def binaries_touched(files: list[str]) -> set[str]:
    # The build Dockerfile and .dockerignore change every image, like Cargo.lock.
    rust = [f for f in files if f.startswith("src/") or f.startswith("Cargo.") or f in TELEBOT_SOURCE]
    if not rust:
        return set()
    if all(f.startswith("src/bin/") for f in rust):
        return {f.split("/")[2] for f in rust} & set(BINARY_DEPLOY)
    return set(BINARY_DEPLOY)


def revert_subject(title: str, n: int) -> str:
    """`revert: <summary> (#n)` within the 72-character subject rule; the
    reverted PR's own type prefix is dropped, a long summary is cut."""
    summary = re.sub(r"^[a-z]+(\([^)]*\))?: ", "", title)
    tail = f" (#{n})"
    room = 72 - len("revert: ") - len(tail)
    if len(summary) > room:
        summary = summary[:room - 1].rstrip() + "\u2026"
    return f"revert: {summary}{tail}"


def rollback_plan(pr: dict, shas: list[str], running_sha: str | None) -> list[str]:
    n = pr["number"]
    sha = running_sha or "<the sha deploy-audit printed before the deploy>"
    subject = shlex.quote(revert_subject(pr["title"], n))
    lines = [f"git switch -c revert/pr-{n} origin/main",
             f"git revert -n {' '.join(shas)}",
             f"git commit -m {subject}   # body: why, and `refs #<incident>`, never `closes`",
             "then pbtb-ship: gate, pr-reviewer, Review-verdict line, PR, gh pr merge <n> --auto --rebase --delete-branch"]
    if any(f.startswith("site/") for f in pr["files"]):
        lines.append("site: pages-publish runs on the push to main")
    touched = binaries_touched(pr["files"])
    if "telebot" in touched:
        lines.append(f"telebot, at once (needs no merge; :latest stays on the bad image, telebot-build skips a "
                     f"source it has built): {BINARY_DEPLOY['telebot'].format(sha=sha)}")
    for b in sorted(touched & AGENT_BINARIES - {"telebot"}):
        lines.append(f"{b}, after the revert merges: {BINARY_DEPLOY[b]}")
    for b in sorted(touched - AGENT_BINARIES):
        lines.append(f"{b}: the owner's call, not the agent's: {BINARY_DEPLOY[b]}")
    if len(lines) == 4:
        lines.append("nothing deployed to redeploy")
    return lines


def gh_json(args: list[str]):
    r = subprocess.run(["gh", *args], capture_output=True, text=True, encoding="utf-8")
    if r.returncode != 0:
        raise RuntimeError(f"gh {' '.join(args[:3])}: {r.stderr.strip()}")
    return json.loads(r.stdout)


def cmd_rollback_plan(a):
    got = gh_json(["pr", "view", str(a.pr), "--json",
                   "number,title,state,mergeCommit,commits,files,closingIssuesReferences"])
    if got["state"] != "MERGED":
        raise RuntimeError(f"#{a.pr} is {got['state']}, not merged")
    pr = {"number": got["number"], "title": got["title"], "merge_sha": got["mergeCommit"]["oid"],
          "headlines": [c["messageHeadline"] for c in got["commits"]],
          "files": [f["path"] for f in got["files"]]}
    git(["fetch", "origin", "main", "-q"])
    if subprocess.run(["git", "merge-base", "--is-ancestor", pr["merge_sha"], "origin/main"]).returncode:
        raise RuntimeError(f"{pr['merge_sha'][:7]} is not on origin/main")
    n = max(len(pr["headlines"]), 1)
    history = [dict(zip(("sha", "parents", "subject"), (s, p.split(), subj)))
               for s, p, subj in (l.split("\t", 2) for l in
                                  git(["log", f"-{n}", "--format=%H%x09%P%x09%s", pr["merge_sha"]]).splitlines())]
    m = SQUASHED.search(history[0]["subject"]) if history else None
    span = 1 if m and int(m.group(1)) == pr["number"] else len(pr["headlines"])
    base = f"{pr['merge_sha']}~{span}"
    range_files = set(git(["diff", "--name-only", base, pr["merge_sha"]]).splitlines())
    shas, why = revert_range(pr, history, range_files)
    owned = codeowners(git(["show", "origin/main:.github/CODEOWNERS"]))
    if not owned:
        raise RuntimeError("could not read .github/CODEOWNERS on origin/main; the code-owned refusal would be off")
    invariants = []
    for issue in got.get("closingIssuesReferences") or []:
        body = gh_json(["issue", "view", str(issue["number"]), "--json", "body"])["body"]
        invariants += ticked_invariants(body)
    refusals = ([why] if why else []) + rollback_refusals(pr, owned, invariants)
    if "telebot" in binaries_touched(pr["files"]):
        pin = a.running_sha
        ancestor = bool(pin) and subprocess.run(
            ["git", "merge-base", "--is-ancestor", pin, base], capture_output=True).returncode == 0
        image_commits = git(["log", "--format=%H", f"{pin}..origin/main", "--", *TELEBOT_SOURCE]).split() \
            if ancestor else []
        problem = telebot_pin_problem(pin, ancestor, shas, image_commits)
        if problem:
            refusals.append(problem)
    print(f"#{pr['number']} {pr['title']}")
    print(f"merge {pr['merge_sha'][:7]}, {len(pr['headlines'])} PR commit(s), {len(pr['files'])} file(s)")
    if refusals:
        print("\nNOT for an agent to roll back alone:")
        for r in refusals:
            print(f"  - {r}")
    print("\nplan:" if not refusals else "\nplan, for the owner to weigh:")
    for line in rollback_plan(pr, shas or ["<range: see above>"], a.running_sha):
        print(f"  {line}")
    raise SystemExit(2 if refusals else 0)


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--profile", default=DEFAULT_PROFILE)
    p.add_argument("--region", default=DEFAULT_REGION)
    p.add_argument("--env", default=DEFAULT_ENV)
    sub = p.add_subparsers(dest="cmd", required=True)

    s = sub.add_parser("rollback-plan", help="the revert range, refusals and redeploy commands for a merged PR")
    s.add_argument("pr", type=int)
    s.add_argument("--running-sha", help="the telebot sha deploy-audit printed before the deploy")
    s.set_defaults(fn=cmd_rollback_plan)

    s = sub.add_parser("bot-status", help="desired vs observed state per bot")
    s.add_argument("bot", nargs="?", default="all")
    s.add_argument("--memory", action="store_true", help="add the latest Container Insights memory sample")
    s.set_defaults(fn=cmd_bot_status)

    s = sub.add_parser("deploy-audit", help="deployed state vs main / terraform / ECR")
    s.set_defaults(fn=cmd_deploy_audit)

    s = sub.add_parser("telebot-logs", help="telebot logs on the NAT host: journald (the wrapper) or, with --app, the application")
    s.add_argument("--since", default="30m")
    s.add_argument("--grep", help="regex; e.g. a redaction ref id like ca7f7c90")
    s.add_argument("--context", type=int, default=6)
    s.add_argument("--tail", type=int, default=60)
    s.add_argument("--env-dump", action="store_true", help="also print the non-secret env keys")
    s.add_argument("--app", action="store_true", help="the application's own output (docker logs telebot) instead of journald")
    s.set_defaults(fn=cmd_telebot_logs)

    s = sub.add_parser("lambda-logs", help="CloudWatch logs of a lambda (task-state | daily-pnl | mcp-http | full name)")
    s.add_argument("name")
    s.add_argument("--since", default="30m")
    s.add_argument("--pattern", help='CloudWatch filter pattern, e.g. "?ERROR ?panic"')
    s.add_argument("--tail", type=int, default=60)
    s.set_defaults(fn=cmd_lambda_logs)

    s = sub.add_parser("codebuild-log", help="tail a CodeBuild build's log, tolerant of corrupt JSON")
    s.add_argument("build_id")
    s.add_argument("--tail", type=int, default=120, help="lines to fetch / show when no error is found")
    s.add_argument("--before", type=int, default=40, help="lines to show before the first error")
    s.add_argument("--after", type=int, default=6)
    s.set_defaults(fn=cmd_codebuild_log)

    s = sub.add_parser("smoke-lambda", help="invoke with an ignored event; expect 200 and no FunctionError (mcp-http: a 401 refusal)")
    s.add_argument("name")
    s.set_defaults(fn=cmd_smoke_lambda)

    s = sub.add_parser("user-show", help="the account row and the identities it holds")
    s.add_argument("user_id")
    s.set_defaults(fn=cmd_user_show)

    s = sub.add_parser("user-create", help="create an account row; optionally bind a Telegram id to it")
    s.add_argument("user_id")
    s.add_argument("--vip", type=int, default=0)
    s.add_argument("--email")
    s.add_argument("--telegram", help="Telegram user id to bind (refused if bound elsewhere)")
    s.set_defaults(fn=cmd_user_create)

    s = sub.add_parser("set-vip", help="change an existing account's level (0..9)")
    s.add_argument("user_id")
    s.add_argument("level", type=int)
    s.set_defaults(fn=cmd_set_vip)

    s = sub.add_parser("set-role", help="make the account the operator's (its bots may be shown publicly) or a member")
    s.add_argument("user_id")
    s.add_argument("role", choices=["operator", "member"])
    s.set_defaults(fn=cmd_set_role)

    s = sub.add_parser("set-public-url", help="set or clear the bot's showcase link (an https bybit.com URL); "
                       "whether it is shown is the console's showcase choice when one was made, "
                       "and only an operator's bots are published")
    s.add_argument("user_id")
    s.add_argument("bot_id")
    s.add_argument("url", help="the bot's Bybit copy-trading page, or `off`")
    s.set_defaults(fn=cmd_set_public_url)

    s = sub.add_parser("user-status", help="activate or suspend an account")
    s.add_argument("user_id")
    s.add_argument("status", choices=["active", "suspended"])
    s.set_defaults(fn=cmd_user_status)

    a = p.parse_args(argv)
    try:
        a.fn(a)
    except RuntimeError as e:
        raise SystemExit(f"error: {e}")


if __name__ == "__main__":
    sys.stdout.reconfigure(encoding="utf-8", errors="replace")
    main()
