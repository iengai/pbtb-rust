#!/usr/bin/env python3
"""Rebuild the NAT in one command, without an egress outage.

The mechanised form of RUNBOOK "Rebuilding the NAT without an egress outage".
That procedure is eight steps, four `terraform.tfvars` edits and five applies,
with a check between each -- and not one of those checks needs a human: every
one of them has a machine-comparable expected result. Left as prose it has to be
walked by hand every time a bootstrap edit lands, which is how two such edits
came to sit committed-but-inert.

    python scripts/ops/nat_rebuild.py                 # the whole window
    python scripts/ops/nat_rebuild.py --dry-run       # preflight + first plan
    python scripts/ops/nat_rebuild.py --from 5        # resume after an abort

Every apply is gated the same way: plan to a file, compare the plan's resource
actions against exactly what that step is allowed to do, and apply the file that
was checked -- never a fresh plan. A step that does not match aborts the window
before touching anything.

Aborting is safe by design. From step 3 on, egress is already on the standby, so
an abort leaves the bots trading through a NAT that is not being rebuilt; the
RUNBOOK's "no rush" state. The script prints the resume command and stops.

What this does NOT remove is the residual gap the design has: the exchange keys
are IP-whitelisted, the EIP has to move with the route, and moving it is a
disassociate + associate. For a few seconds each way, egress leaves through an
address the exchange rejects. Twice per window. That is an AWS API property, not
something automation can paper over.
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
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from pbtb_ops import aws, cfg, ssm, utc  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]
TFDIR = ROOT / "terraform" / "envs" / "dev"
TFVARS = TFDIR / "terraform.tfvars"
ROUTE_TABLE = "rtb-025d0f9c32fa2562b"
STANDBY_TAG = "scalable-cluster-dev-nat-standby"
# The standby is count-gated by nat_standby_enabled, so its plan address is indexed.
STANDBY_ADDR = "module.network.aws_instance.nat_standby[0]"
EIP_TAG = "scalable-cluster-dev-nat-eip"
DEPLOY_WORKFLOW = "telebot-deploy.yml"
GH_OWNER = "iengai"  # the only account with push/dispatch rights on this repo

TF = shutil.which("terraform") or "terraform"
GH = shutil.which("gh") or "gh"


class Abort(Exception):
    """A gate said no. The window stops here, and says how to pick it up."""


# ---------------------------------------------------------------- plumbing

def say(msg: str, *, step: int | None = None):
    prefix = f"[{step}] " if step else "    "
    print(f"{prefix}{msg}", flush=True)


def run(cmd: list[str], cwd: Path | None = None, env_extra: dict | None = None) -> str:
    env = dict(os.environ, MSYS_NO_PATHCONV="1", PYTHONUTF8="1", **(env_extra or {}))
    r = subprocess.run(cmd, cwd=cwd, capture_output=True, env=env)
    out = r.stdout.decode("utf-8", "replace")
    if r.returncode != 0:
        err = r.stderr.decode("utf-8", "replace").strip()
        raise Abort(f"{cmd[0]} {cmd[1] if len(cmd) > 1 else ''} failed:\n{err or out}")
    return out


def terraform(args: list[str], profile: str) -> str:
    return run([TF, *args], cwd=TFDIR, env_extra={"AWS_PROFILE": profile})


# ---------------------------------------------------------------- tfvars switches

def switches() -> tuple[bool, str]:
    s = TFVARS.read_text(encoding="utf-8")
    enabled = re.search(r"^nat_standby_enabled\s*=\s*(true|false)", s, re.M)
    active = re.search(r'^nat_egress_active\s*=\s*"(primary|standby)"', s, re.M)
    if not enabled or not active:
        raise Abort(f"{TFVARS} has no nat_standby_enabled / nat_egress_active pair")
    return enabled.group(1) == "true", active.group(1)


def set_switches(enabled: bool, active: str):
    """Rewrite the two switches in the file.

    They are edited here rather than passed with `-var` on purpose: every apply
    in this window has to agree on them, and one scoped apply that forgot the
    flag would re-point the default route at the instance it is destroying.
    """
    s = TFVARS.read_text(encoding="utf-8")
    s, n1 = re.subn(r"^nat_standby_enabled(\s*)=(\s*)(?:true|false)",
                    lambda m: f"nat_standby_enabled{m.group(1)}={m.group(2)}{str(enabled).lower()}", s, flags=re.M)
    s, n2 = re.subn(r'^nat_egress_active(\s*)=(\s*)"(?:primary|standby)"',
                    lambda m: f'nat_egress_active{m.group(1)}={m.group(2)}"{active}"', s, flags=re.M)
    if (n1, n2) != (1, 1):
        raise Abort(f"expected one match each in terraform.tfvars, replaced {n1}/{n2}")
    TFVARS.write_text(s, encoding="utf-8", newline="")
    say(f"tfvars: nat_standby_enabled={str(enabled).lower()}  nat_egress_active=\"{active}\"")


# ---------------------------------------------------------------- the gated apply

def plan_apply(step: int, targets: list[str], expect: dict[str, set[str]],
               optional: dict[str, set[str]], profile: str, dry_run: bool):
    """Plan to a file, assert the plan is exactly this step's change, apply that file.

    Comparing resource ADDRESSES and ACTIONS rather than the "N to add, N to
    change" line: the counts are the same for several different mistakes, and a
    replacement of the wrong instance counts identically to a replacement of the
    right one.
    """
    plan_file = TFDIR / f".nat-rebuild-{step}.tfplan"
    args = ["plan", "-input=false", "-no-color", f"-out={plan_file.name}"]
    args += [f"-target={t}" for t in targets]
    terraform(args, profile)

    shown = json.loads(terraform(["show", "-json", plan_file.name], profile))
    got = {rc["address"]: set(rc["change"]["actions"])
           for rc in shown.get("resource_changes", [])
           if rc["change"]["actions"] != ["no-op"]}

    unexpected = {a: v for a, v in got.items() if a not in expect and a not in optional}
    missing = {a: v for a, v in expect.items() if a not in got}
    wrong = {a: (got[a], expect[a]) for a in expect if a in got and got[a] != expect[a]}
    wrong |= {a: (got[a], optional[a]) for a in optional if a in got and got[a] != optional[a]}
    if unexpected or missing or wrong:
        plan_file.unlink(missing_ok=True)
        raise Abort("plan does not match what this step is allowed to do:\n"
                    + "".join(f"  unexpected {a}: {sorted(v)}\n" for a, v in unexpected.items())
                    + "".join(f"  missing    {a}: expected {sorted(v)}\n" for a, v in missing.items())
                    + "".join(f"  wrong      {a}: got {sorted(g)}, expected {sorted(e)}\n"
                              for a, (g, e) in wrong.items()))

    for addr, actions in sorted(got.items()):
        say(f"plan ok: {addr} -> {'+'.join(sorted(actions))}")
    if dry_run:
        plan_file.unlink(missing_ok=True)
        say("dry-run: not applying")
        return
    terraform(["apply", "-input=false", "-no-color", plan_file.name], profile)
    plan_file.unlink(missing_ok=True)
    say("applied")


# ---------------------------------------------------------------- AWS reads

def instance_by_tag(tag: str, a) -> str | None:
    r = aws(["ec2", "describe-instances", "--filters", f"Name=tag:Name,Values={tag}",
             "Name=instance-state-name,Values=running",
             "--query", "Reservations[].Instances[].InstanceId"], a.profile, a.region)
    return r[0] if r else None


def eni_of(instance: str, a) -> str:
    return aws(["ec2", "describe-instances", "--instance-ids", instance,
                "--query", "Reservations[0].Instances[0].NetworkInterfaces[0].NetworkInterfaceId"],
               a.profile, a.region)


def eip_holder(a) -> tuple[str, str]:
    r = aws(["ec2", "describe-addresses", "--filters", f"Name=tag:Name,Values={EIP_TAG}",
             "--query", "Addresses[0].[PublicIp,InstanceId]"], a.profile, a.region)
    return (r[0] or "?"), (r[1] or "")


def default_route_eni(a) -> str:
    r = aws(["ec2", "describe-route-tables", "--route-table-ids", ROUTE_TABLE,
             "--query", "RouteTables[0].Routes[?DestinationCidrBlock=='0.0.0.0/0'].NetworkInterfaceId"],
            a.profile, a.region)
    return (r or [""])[0] or ""


def running_bots(a) -> dict[str, str]:
    """{taskArn: startedAt} -- the evidence that no bot restarted during the window."""
    c = cfg(a.env)
    arns = aws(["ecs", "list-tasks", "--cluster", c["cluster"], "--desired-status", "RUNNING"],
               a.profile, a.region)["taskArns"]
    if not arns:
        return {}
    tasks = aws(["ecs", "describe-tasks", "--cluster", c["cluster"], "--tasks", *arns],
                a.profile, a.region)["tasks"]
    return {t["taskArn"].split("/")[-1]: t.get("startedAt", "") for t in tasks}


def assert_egress_on(instance: str, who: str, a):
    ip, holder = eip_holder(a)
    if holder != instance:
        raise Abort(f"EIP {ip} is on {holder or '(nothing)'}, expected the {who} NAT {instance}")
    eni = eni_of(instance, a)
    route = default_route_eni(a)
    if route != eni:
        raise Abort(f"private 0.0.0.0/0 points at {route or '(nothing)'}, expected the {who} ENI {eni}")
    say(f"egress on the {who} NAT: EIP {ip} -> {instance}, route -> {eni}")


def assert_forwards(instance: str, a):
    out = ssm(instance, ["cloud-init status --wait || true",
                         "sysctl net.ipv4.ip_forward",
                         "iptables -t nat -S POSTROUTING"], a, timeout_s=300)
    if "status: done" not in out:
        raise Abort(f"cloud-init has not finished on {instance}:\n{out}")
    if "net.ipv4.ip_forward = 1" not in out:
        raise Abort(f"ip_forward is not 1 on {instance}:\n{out}")
    if "-j MASQUERADE" not in out:
        raise Abort(f"no MASQUERADE rule on {instance}:\n{out}")
    say(f"{instance} forwards: cloud-init done, ip_forward=1, MASQUERADE present")


def gh_active_account() -> str:
    """The gh account currently in use.

    Two accounts are logged in on this machine and only the repo owner can
    dispatch a workflow, so the switch has to be made and then undone -- leaving
    the wrong one active breaks the next unrelated gh call.
    """
    out = run([GH, "auth", "status"])
    account = ""
    for line in out.splitlines():
        m = re.search(r"account (\S+)", line)
        if m:
            account = m.group(1)
        elif "Active account: true" in line and account:
            return account
    return ""


def gh_run_ids() -> list[str]:
    runs = json.loads(run([GH, "run", "list", "--workflow", DEPLOY_WORKFLOW,
                           "--limit", "10", "--json", "databaseId"], cwd=ROOT))
    return [str(r["databaseId"]) for r in runs]


# ---------------------------------------------------------------- the steps

def step1_standby_up(a):
    set_switches(enabled=True, active="primary")
    plan_apply(1, ["module.network.aws_instance.nat_standby"],
               {STANDBY_ADDR: {"create"}}, {}, a.profile, a.dry_run)


def step2_standby_verify(a):
    sb = instance_by_tag(STANDBY_TAG, a)
    if not sb:
        raise Abort(f"no running instance tagged {STANDBY_TAG}")
    say(f"standby is {sb}")
    assert_forwards(sb, a)


def step3_egress_to_standby(a):
    set_switches(enabled=True, active="standby")
    plan_apply(3, ["module.network.aws_route.private_nat", "module.network.aws_eip_association.nat"],
               {"module.network.aws_route.private_nat": {"update"},
                "module.network.aws_eip_association.nat": {"create", "delete"}}, {}, a.profile, a.dry_run)


def step4_egress_verify(a):
    assert_egress_on(instance_by_tag(STANDBY_TAG, a), "standby", a)


def step5_rebuild_primary(a):
    plan_apply(5, ["module.network.aws_instance.nat", "aws_ssm_parameter.telebot_base_env"],
               {"module.network.aws_instance.nat": {"create", "delete"}},
               {"aws_ssm_parameter.telebot_base_env": {"update"}}, a.profile, a.dry_run)


def step6_telebot_up(a):
    c = cfg(a.env)
    primary = instance_by_tag(c["nat_tag_name"], a)
    if not primary:
        raise Abort(f"no running instance tagged {c['nat_tag_name']} after the rebuild")
    say(f"new primary is {primary}")
    assert_forwards(primary, a)

    # user_data deliberately carries no app config, so the new host has no
    # /etc/telebot/telebot.env and telebot cannot come up on its own.
    was_active = gh_active_account()
    if was_active != GH_OWNER:
        run([GH, "auth", "switch", "--user", GH_OWNER])
    try:
        before_ids = gh_run_ids()
        run([GH, "workflow", "run", DEPLOY_WORKFLOW, "--ref", "main",
             "-f", "tag=latest", "-f", "passivbot_revisions=latest"], cwd=ROOT)
        say("dispatched telebot-deploy, waiting for the run to appear")
        run_id = ""
        for _ in range(20):
            time.sleep(5)
            new_ids = [i for i in gh_run_ids() if i not in before_ids]
            if new_ids:
                run_id = new_ids[0]
                break
        if not run_id:
            raise Abort("telebot-deploy was dispatched but no new run appeared within 100s")
        say(f"watching run {run_id}")
        subprocess.run([GH, "run", "watch", run_id, "--exit-status"], cwd=ROOT)
        got = json.loads(run([GH, "run", "view", run_id, "--json", "conclusion"], cwd=ROOT))
        if got["conclusion"] != "success":
            raise Abort(f"telebot-deploy run {run_id} concluded {got['conclusion']}")
        say("telebot-deploy succeeded")
    finally:
        if was_active and was_active != GH_OWNER:
            run([GH, "auth", "switch", "--user", was_active])

    out = ssm(primary, ["docker ps --filter name=telebot --format '{{.Status}}'"], a)
    if not out.strip().startswith("Up"):
        raise Abort(f"telebot is not up on {primary}: {out!r}")
    say(f"telebot: {out.strip()}")


def step7_egress_to_primary(a):
    set_switches(enabled=True, active="primary")
    plan_apply(7, ["module.network.aws_route.private_nat", "module.network.aws_eip_association.nat"],
               {"module.network.aws_route.private_nat": {"update"},
                "module.network.aws_eip_association.nat": {"create", "delete"}}, {}, a.profile, a.dry_run)
    assert_egress_on(instance_by_tag(cfg(a.env)["nat_tag_name"], a), "primary", a)


def step8_standby_down(a):
    set_switches(enabled=False, active="primary")
    plan_apply(8, ["module.network.aws_instance.nat_standby"],
               {STANDBY_ADDR: {"delete"}}, {}, a.profile, a.dry_run)


STEPS = [
    (1, "bring the standby up (live path untouched)", step1_standby_up),
    (2, "verify the standby forwards", step2_standby_verify),
    (3, "flip egress onto the standby", step3_egress_to_standby),
    (4, "confirm the bots are out through the standby", step4_egress_verify),
    (5, "rebuild the primary NAT", step5_rebuild_primary),
    (6, "bring telebot back on the new primary", step6_telebot_up),
    (7, "flip egress back to the primary", step7_egress_to_primary),
    (8, "destroy the standby", step8_standby_down),
]


# ---------------------------------------------------------------- entry point

def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--env", default="dev")
    p.add_argument("--profile", default="dev")
    p.add_argument("--region", default="ap-northeast-1")
    p.add_argument("--from", dest="start", type=int, default=1,
                   help="resume at this step (see the abort message of the previous run)")
    p.add_argument("--dry-run", action="store_true",
                   help="preflight and plan step 1 only; applies nothing. Later steps cannot be "
                        "planned in advance -- their plans assume resources step 1 would create.")
    a = p.parse_args()

    enabled, active = switches()
    say(f"tfvars now: nat_standby_enabled={str(enabled).lower()}  nat_egress_active=\"{active}\"")
    before = running_bots(a)
    say(f"preflight: {len(before)} bot task(s) RUNNING")
    for tid, started in sorted(before.items()):
        say(f"  {tid[:8]}  started {utc(started)}")
    if a.dry_run:
        say("dry-run: rehearsing step 1 only")

    original = TFVARS.read_text(encoding="utf-8")
    try:
        for n, title, fn in STEPS:
            if n < a.start:
                continue
            print(f"\n=== step {n}: {title} ===", flush=True)
            fn(a)
            if a.dry_run:
                say("dry-run: stopping after the first planned step")
                break
    except Abort as e:
        _, active_now = switches()
        print(f"\nABORTED at that step: {e}", file=sys.stderr)
        if a.dry_run:
            TFVARS.write_text(original, encoding="utf-8", newline="")
            print("dry-run: terraform.tfvars restored", file=sys.stderr)
        else:
            print(f"\ntfvars is left at nat_egress_active=\"{active_now}\" on purpose: it has to keep\n"
                  f"describing reality. Egress is {'on the standby, so the bots are trading and there is no rush'
                                                   if active_now == 'standby' else 'on the primary'}.\n"
                  f"Fix the cause, then: python scripts/ops/nat_rebuild.py --from <step>", file=sys.stderr)
        raise SystemExit(1)

    if a.dry_run:
        TFVARS.write_text(original, encoding="utf-8", newline="")
        say("\ndry-run: terraform.tfvars restored, nothing applied")
        return

    after = running_bots(a)
    restarted = [t for t, s in after.items() if before.get(t) != s] + [t for t in before if t not in after]
    print("\n=== window closed ===")
    say(f"bot tasks RUNNING: {len(after)} (was {len(before)})")
    if restarted:
        say(f"WARNING these tasks are not the ones that started the window: {', '.join(t[:8] for t in restarted)}")
    else:
        say("no bot restarted: same task ids, same start times")
    say("commit terraform.tfvars -- it is back at the steady state (false / \"primary\")")


if __name__ == "__main__":
    main()
