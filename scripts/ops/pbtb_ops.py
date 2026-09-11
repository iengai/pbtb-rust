#!/usr/bin/env python3
"""Read-only operations CLI for the pbtb-rust dev deployment.

One file, several subcommands, so every skill and every human can get the
same answers with one command instead of re-deriving where things live:

  bot-status [BOT_ID|all] [--memory]   desired vs observed state per bot
  deploy-audit                         what is deployed vs what main/terraform say
  telebot-logs [--since 30m] [--grep X] [--app] telebot logs on the NAT host (journald, or the app with --app)
  lambda-logs NAME [--since 30m] [--pattern X]
  codebuild-log BUILD_ID [--tail N]    tolerant of the corrupt JSON CloudWatch emits
  smoke-lambda NAME                    invoke with an event the handler ignores
  user-show USER_ID                    the account row and the identities it holds
  user-create USER_ID [--vip N] [--email E] [--telegram TG_ID]
                                       create an account row (and bind a Telegram id)
  set-vip USER_ID LEVEL                change an existing account's level
  user-status USER_ID active|suspended

Everything here is read-only except `smoke-lambda`, which invokes a function
with an event its guard clause discards before any side effect, and the
`user-*` / `set-vip` commands, which write account rows to the bots table
(each is a conditional write: create refuses an existing account, the
updates refuse a missing one).

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
import shutil
import subprocess
import sys
import time
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
                            f"-> takes effect on the next Stop/Run (or auto-restart)")
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
        print(f"{eng or '?'}: {fam}:{td['revision']}  image={td['containerDefinitions'][0]['image'].split(':')[-1]}  memory={td.get('memory')}")
        if eng is None:
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


def cmd_smoke_lambda(a):
    c = cfg(a.env)
    fname = c["lambdas"].get(a.name, a.name)
    payload = '{"version":"0","source":"pbtb.smoke-test","detail-type":"SmokeTest","detail":{}}'
    out = aws(["lambda", "invoke", "--function-name", fname, "--cli-binary-format", "raw-in-base64-out",
               "--payload", payload, os.devnull], a.profile, a.region)
    status, ferr = out.get("StatusCode"), out.get("FunctionError")
    print(f"{fname}: StatusCode={status} FunctionError={ferr}")
    if status != 200 or ferr:
        raise SystemExit(1)


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
        for k in ("vip_level", "status", "email", "created_at", "updated_at"):
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


def cmd_user_status(a):
    c = cfg(a.env)
    if dyn_update_user(c, a.user_id, "status", {"S": a.status}, a):
        print(f"{a.user_id} status={a.status}")
    else:
        raise RuntimeError(f"no account row for {a.user_id} (user-create first)")


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--profile", default=DEFAULT_PROFILE)
    p.add_argument("--region", default=DEFAULT_REGION)
    p.add_argument("--env", default=DEFAULT_ENV)
    sub = p.add_subparsers(dest="cmd", required=True)

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

    s = sub.add_parser("smoke-lambda", help="invoke with an ignored event; expect 200 and no FunctionError")
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
