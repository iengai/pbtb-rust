#!/usr/bin/env python3
"""Curate the catalogue: each template's `lab` record, naming properties and titles.

A template's JSON carries two blocks of ours, split by who may read them:

* ``pbtb`` is the console's: id, titles, the naming properties
  (template_naming.py), description, strategies, level gate. Anything under it
  can reach a user.
* ``lab`` is ours: the strategy-lab config the template was harvested as, the
  optimizer run and population member, the seeds that run warm-started from,
  the genome it belongs to, the lab's verdicts and notes. No surface reads it,
  and ``BotConfig::from_template`` leaves it out of a bot's copy, which the MCP
  ``get_bot_config`` tool returns whole.

Genome
------
An optimizer run warm-starts from earlier winners (the lab's
``configs/seeds_<tier>_<iter>/``), so one tuning is carried across capital
tiers and refined over several iterations. ``genome`` is the member a family
descends from, ``branch`` the refinement within it:

* ``6501db3f96`` (``cap1000_iter2_maxreturn``). The cap700 iter2 winners copied
  it (``origin``); iter3 added the 2.5 cap per alt coin (``alt-cap``);
  ``cap500_iter5_winner`` refit it at $500 and seeded cap700 and cap1000 from
  there (``record-genome``); the seed-isolation arm and the v8.1.0 cap1000
  rounds widened its exits (``let-profits-run``). Every member sits within 0.30
  normalised parameter distance of the others.
* Every other lab template is at least 0.43 from all of them and is its own
  genome, named by its own member.
* The XRP templates predate the lab and carry none.

The tables below are keyed by the readable id each template had before ids went
opaque, kept as ``lab.readable_id``. ``tier`` / ``iter`` / ``source`` are the
lab's own coordinates and ``original_name`` the optimizer-run name before that,
so a template can still be found in the lab's NOTES.md and STRATEGIES.md.

Each run also re-derives what the config and the lineage decide (``style``,
``engine``, ``generation``) and recomposes every title from the naming
properties across the templates listed together, so a suffix appears or goes
as templates are added and retired. Re-run it after either.

Nothing passivbot reads is touched. The backtest artifacts' ``source_sha``
follows the rewritten bytes where it matched the old ones, so the backtests
are not re-run.

Usage::

    python scripts/annotate_templates.py                  # dry run
    python scripts/annotate_templates.py --apply --profile dev
"""

from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import sys
from pathlib import Path

from backtest_templates import write_index, write_json
from rename_predefined import BUCKET, CATALOG, PREFIX, RETIRED_PREFIX, aws, body_bytes
from template_naming import FACETS, IDS, engine_of, style_of, titles

REPO_ROOT = Path(__file__).resolve().parents[1]
SITE_DIR = REPO_ROOT / "site" / "templates"
LAB_KEY = "lab"
OURS = ("pbtb", LAB_KEY)
LEAD = ("name", "title", "title_zh", *FACETS)
ARTIFACT_FIELDS = ("title", "title_zh", "style", "generation")

GENOME = "6501db3f96"
ORIGINAL = {readable: old for old, (readable, _, _) in CATALOG.items()}


def lineage(source, tier, iteration, member, *, run=None, seeds=(), genome=None,
            branch=None, variant=None) -> dict:
    entry = {
        "source": source,
        "tier": tier,
        "iter": iteration,
        "run": run,
        "member": member,
        "seeds": list(seeds),
        "genome": genome or member,
        "branch": branch,
        "variant": variant,
    }
    return {k: v for k, v in entry.items() if v not in (None, [])}


HYSTERESIS = "live.forager_score_hysteresis_pct 0.02 -> 0.05 on the iter1 winner"
CAP1000_ITER2 = ["cap1000_iter1_winner", "cap700_iter1_winner", "cap500_iter1_winner"]
CAP700_ITER2 = ["cap700_iter1_winner", "cap500_iter1_winner", "cap1000_iter1_winner",
                "cap1000_iter2_maxreturn"]
CAP500_ITER6 = ["cap500_iter5_winner", "cap700_iter3_winner", "cap700_iter2_winner",
                "cap1000_iter3_winner"]
CAP1000_ITER7 = ["cap1000_iter6_winner", "cap1000_iter6_maxreturn", "cap1000_iter5_winner",
                 "cap700_iter6_winner"]

LINEAGE: dict[str, dict] = {
    # --- singletons ---------------------------------------------------------
    "bybit-mix3-100u-steady-v7": lineage(
        "cap100_iter1_winner", "cap100", 1, "d13023b9cd", run="a88ba55c"),
    "bybit-mix8-300u-bold-v7": lineage(
        "cap300_iter1_winner", "cap300", 1, "d1ee2f5088", run="5446a8c7"),
    "bybit-mix10-500u-steady-v7": lineage(
        "cap500_iter1_winner_hyst05", "cap500", 1, "5a853b692b", variant=HYSTERESIS),
    "bybit-mix10-1000u-steady-v7": lineage(
        "cap1000_iter1_winner_hyst05", "cap1000", 1, "6a7daafecd", variant=HYSTERESIS),
    "bybit-mix10-500u-guard-v7": lineage(
        "cap500_iter3_lowdd", "cap500", 3, "beacf3e222", run="2041bf4c",
        seeds=["cap500_iter1_winner", "cap500_iter1_maxreturn", "cap500_iter1_lowdd",
               "cap1000_iter2_maxreturn", "cap1000_iter2_balanced"]),
    "bybit-mix10-1000u-balanced-v7-a": lineage(
        "cap1000_iter2_balanced", "cap1000", 2, "8b41f145d3", seeds=CAP1000_ITER2),
    # --- the 6501db3f96 family ----------------------------------------------
    "bybit-mix10-1000u-bold-v7": lineage(
        "cap1000_iter2_maxreturn", "cap1000", 2, GENOME, seeds=CAP1000_ITER2,
        genome=GENOME, branch="origin"),
    "bybit-mix10-700u-bold-v7-b": lineage(
        "cap700_iter2_winner", "cap700", 2, "bae05ab2a1", run="49c87e56",
        seeds=CAP700_ITER2, genome=GENOME, branch="origin"),
    "bybit-mix10-700u-bold-v7-d": lineage(
        "cap700_iter2_maxreturn", "cap700", 2, "e84bc4b1f0", run="49c87e56",
        seeds=CAP700_ITER2, genome=GENOME, branch="origin"),
    "bybit-mix10-700u-bold-v7-c": lineage(
        "cap700_iter3_winner", "cap700", 3, "89e6729e28", run="46e69713",
        seeds=["cap700_iter2_winner", "cap700_iter2_maxreturn", "cap1000_iter2_balanced",
               "cap1000_iter2_maxreturn", "cap700_iter1_winner"],
        genome=GENOME, branch="alt-cap"),
    "bybit-mix10-1000u-balanced-v7-c": lineage(
        "cap1000_iter3_winner", "cap1000", 3, "710ed44f45", run="23dd4d46",
        seeds=["cap1000_iter2_maxreturn", "cap1000_iter2_balanced",
               "cap1000_iter1_winner_hyst05", "cap700_iter2_winner", "cap700_iter3_winner"],
        genome=GENOME, branch="alt-cap"),
    "bybit-mix10-500u-bold-v7-a": lineage(
        "cap500_iter5_winner", "cap500", 5, "043cbfef98", run="5f1e80fc",
        seeds=["cap500_iter1_winner_hyst05", "cap500_iter3_lowdd", "cap700_iter2_winner",
               "cap700_iter3_winner", "cap1000_iter2_balanced"],
        genome=GENOME, branch="record-genome"),
    "bybit-mix10-700u-bold-v7-a": lineage(
        "cap700_iter5_winner", "cap700", 5, "046ee9dca3", run="b1160059",
        seeds=["cap500_iter5_winner", "cap700_iter3_winner", "cap700_iter2_winner",
               "cap700_iter2_maxreturn", "cap1000_iter3_winner"],
        genome=GENOME, branch="record-genome"),
    "bybit-mix10-1000u-balanced-v7-b": lineage(
        "cap1000_iter4_winner", "cap1000", 4, "05c86b8b67", run="a104579b",
        seeds=["cap500_iter5_winner", "cap1000_iter3_winner", "cap1000_iter2_maxreturn",
               "cap700_iter3_winner"],
        genome=GENOME, branch="record-genome"),
    "bybit-mix10-500u-balanced-v7": lineage(
        "cap500_iter6_maxreturn", "cap500", 6, "542da0d8f5", run="8510283d",
        seeds=CAP500_ITER6, genome=GENOME, branch="let-profits-run"),
    "bybit-mix10-500u-bold-v7-b": lineage(
        "cap500_iter6_highreturn", "cap500", 6, "75fdbe22e6", run="8510283d",
        seeds=CAP500_ITER6, genome=GENOME, branch="let-profits-run"),
    "bybit-mix10-1000u-balanced-v8": lineage(
        "cap1000_iter7_winner", "cap1000", 7, "8914adeb2d", run="1b750084",
        seeds=CAP1000_ITER7, genome=GENOME, branch="let-profits-run"),
    "bybit-mix10-1000u-bold-v8": lineage(
        "cap1000_iter7_highreturn", "cap1000", 7, "2a19337fbf", run="1b750084",
        seeds=CAP1000_ITER7, genome=GENOME, branch="let-profits-run"),
}

# A v8.1.0 template converted from a v7 one (`passivbot tool migrate-config-v7`)
# is the same tuning, not a new optimizer result, and inherits its lineage.
MIGRATED: dict[str, str] = {
    "bybit-mix3-100u-steady-v8": "bybit-mix3-100u-steady-v7",
    "bybit-mix8-300u-bold-v8": "bybit-mix8-300u-bold-v7",
    "bybit-mix10-700u-bold-v8": "bybit-mix10-700u-bold-v7-a",
    "bybit-xrp-100u-bold-v8": "bybit-xrp-100u-bold-v7",
    "bybit-xrp-100u-extreme-v8": "bybit-xrp-100u-extreme-v7-a",
}

# readable id -> (status, the verdict line appended to the notes).
VERDICTS: dict[str, tuple[str | None, str]] = {
    "bybit-mix10-1000u-balanced-v7-a": (
        "not-deployable",
        "2026-09-04 重测:2026-04-25→09-04(含 2026-06-06 真实崩盘)于 06-05 清算,完成率 0.315;"
        "登记册 CRASH dd 0.205 没有预测到。实验室降级为不可部署。",
    ),
    "bybit-mix10-1000u-steady-v7": (
        None,
        "2026-09-04 重测:2026-04-25→09-04(含 2026-06-06 真实崩盘)+10.4% / dd 36.9%,同批最差;"
        "dd 0.154 的登记册画像在 2026 失真,黏滞选币在崩盘中拖累。",
    ),
    "bybit-mix10-500u-bold-v7-b": (
        "fragile",
        "数据漂移重测:30.37→15.79×、dd 0.370→0.475、回本 7d→59d;对 hyst 与数据都敏感,"
        "判定为脆弱配置,不建议实盘。",
    ),
    "bybit-xrp-100u-extreme-v7-a": (
        "fails-crash-gate",
        "2026-09-04 重测(sb=5000):FULL 2024-03→2025-10 清算(完成率 0.049),2025-10-10 崩盘窗清算;"
        "任何 twel 都过不了崩盘闸,问题在深马丁网格结构。实盘存活是路径运气,不是配置安全。",
    ),
}

# A verdict a user should be told, appended to the public description. The
# profile it moved the template to is stored in `pbtb.profile`.
WARNINGS: dict[str, str] = {
    "bybit-mix10-1000u-steady-v7":
        "⚠️ 2026-04-25→09-04 回测(含 2026-06-06 真实崩盘)回撤 36.9%,不是低回撤配置",
}


def trading(config: dict) -> dict:
    return {k: v for k, v in config.items() if k not in OURS}


def readable_of(raw: dict, tid: str) -> str:
    return (raw.get(LAB_KEY) or {}).get("readable_id") or tid


def annotate(raw: dict, readable: str) -> dict:
    meta = dict(raw.get("pbtb") or {})
    previous = raw.get(LAB_KEY) or {}

    notes = previous.get("notes") or meta.get("description") or ""
    status, verdict = VERDICTS.get(readable, (None, None))
    if verdict and verdict not in notes:
        notes = f"{notes}\n{verdict}" if notes else verdict

    if readable in MIGRATED:
        parent = MIGRATED[readable]
        entry = {**LINEAGE.get(parent, {}), "migrated_from": IDS.get(parent, parent)}
    else:
        entry = dict(LINEAGE.get(readable, {}))
    block = {
        "original_name": ORIGINAL.get(readable),
        "readable_id": previous.get("readable_id"),
        **entry,
        "status": status,
        "notes": notes or None,
    }

    warning = WARNINGS.get(readable)
    description = meta.get("description") or ""
    if warning and warning not in description:
        meta["description"] = f"{description}\n{warning}" if description else warning

    meta["style"] = style_of(raw)
    meta["engine"] = engine_of(raw)
    if "iter" in entry:
        meta["generation"] = entry["iter"]
    else:
        meta.pop("generation", None)

    out = {k: v for k, v in raw.items() if k != LAB_KEY}
    out["pbtb"] = meta
    out[LAB_KEY] = {k: v for k, v in block.items() if v is not None}
    return out


def ordered(meta: dict) -> dict:
    return {**{k: meta[k] for k in LEAD if k in meta},
            **{k: v for k, v in meta.items() if k not in LEAD}}


def curate(group: dict[str, dict]) -> dict[str, dict]:
    """id -> the annotated body, for templates listed together."""
    out = {tid: annotate(raw, readable_of(raw, tid)) for tid, raw in group.items()}
    for tid, (title, title_zh) in titles({t: b["pbtb"] for t, b in out.items()}).items():
        out[tid]["pbtb"]["title"] = title
        out[tid]["pbtb"]["title_zh"] = title_zh
    for body in out.values():
        body["pbtb"] = ordered(body["pbtb"])
    return out


def sha(body: bytes) -> str:
    return hashlib.sha256(body).hexdigest()


def refresh_artifact(tid: str, old: bytes, new: bytes, meta: dict, apply: bool) -> bool:
    path = SITE_DIR / f"{tid}.json"
    if not path.exists():
        return False
    art = json.loads(path.read_text(encoding="utf-8"))
    changed = False
    if art.get("source_sha") == sha(old):
        art["source_sha"] = sha(new)
        changed = True
    else:
        print("    artifact was already stale; source_sha left for backtest_templates.py")
    for field in ARTIFACT_FIELDS:
        if art.get(field) != meta.get(field):
            art[field] = meta.get(field)
            changed = True
    if changed and apply:
        write_json(path, art)
    return changed


def main() -> int:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("--apply", action="store_true", help="write (default is a dry run)")
    parser.add_argument("--profile", default=None, help="AWS CLI profile")
    args = parser.parse_args()

    unplaced = [r for r in IDS
                if r not in LINEAGE and r not in MIGRATED and not r.startswith("bybit-xrp-")]
    if unplaced:
        print(f"error: no lineage for {', '.join(unplaced)}", file=sys.stderr)
        return 1

    artifacts = False
    for prefix in (PREFIX, RETIRED_PREFIX):
        listing = aws(["s3", "ls", f"s3://{BUCKET}/{prefix}"], args.profile)
        keys = sorted(line.split()[-1] for line in listing.splitlines()
                      if line.strip().endswith(".json"))
        group: dict[str, dict] = {}
        before: dict[str, bytes] = {}
        for key in keys:
            tid = key.removesuffix(".json")
            text = aws(["s3", "cp", f"s3://{BUCKET}/{prefix}{key}", "-"], args.profile)
            before[tid] = text.encode("utf-8")
            group[tid] = json.loads(text)

        print(f"{prefix} ({len(keys)}):")
        for tid, out in sorted(curate(group).items()):
            assert trading(out) == trading(group[tid]), f"{tid}: would change what passivbot reads"
            old, new = before[tid], body_bytes(out)
            block = out[LAB_KEY]
            family = block.get("genome", "—")
            if block.get("branch"):
                family += f"/{block['branch']}"
            mark = " (current)" if new == old else ""
            print(f"  {tid:34} {family:28} {out['pbtb'].get('title', '')}"
                  f" {block.get('status', '')}{mark}")
            if new == old:
                continue
            if prefix == PREFIX:
                artifacts |= refresh_artifact(tid, old, new, out["pbtb"], args.apply)
            if not args.apply:
                continue
            tmp = REPO_ROOT / ".cache" / "annotate_templates" / f"{tid}.json"
            tmp.parent.mkdir(parents=True, exist_ok=True)
            tmp.write_bytes(new)
            aws(["s3", "cp", str(tmp), f"s3://{BUCKET}/{prefix}{tid}.json",
                 "--content-type", "application/json"], args.profile)

    if artifacts and args.apply:
        write_index()
        print("site/templates: artifacts refreshed, index rebuilt")
    if not args.apply:
        print("\n(dry run) re-run with --apply to write.")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except subprocess.CalledProcessError as exc:
        print(f"error: {' '.join(exc.cmd[:4])} failed: {exc.stderr!r}", file=sys.stderr)
        raise SystemExit(1)
