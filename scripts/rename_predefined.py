#!/usr/bin/env python3
"""Give every `predefined/` template a stable id and a display title.

The names the templates were first uploaded under encoded the optimizer run
that produced them (``iter7``, ``hyst05``, ``r46x-lq``) and, worse, what that
run was called when it won its sweep (``winner``, ``maxreturn``, ``highreturn``)
— a return claim inside an identifier a user reads. Four unrelated spellings
had accumulated, from ``xrp241201250401`` to
``bybit-xrp-241201251009-r46x-lq-v810``.

This script moves the store to one grammar, and splits identity from wording:

    id     bybit-<universe>-<capital>-<profile>-<engine line>[-<letter>]
    title  10-coin basket · Balanced · $1k        (pbtb.title)
           十币组合 · 平衡 · $1k                    (pbtb.title_zh)

The id is the S3 key and never changes; the titles are data inside the config
and can be rewritten without moving anything. Each field of the id comes from
the config itself: the coin universe and the tuned capital are the backtest's,
the engine line is ``config_version``'s major, and the profile is the tier the
measured worst drawdown fell in when the template was published. The same
tuning on two engine lines keeps one profile — the more cautious of the two
tiers — so a user who switches lines is still looking at the same product. A
letter disambiguates templates that agree on all of it, assigned tamest-first.

The mapping below is frozen rather than recomputed: it is the record of which
old name became which id, which the config-switch history rows still hold.

What it rewrites, per template:

* S3 ``predefined/<old>.json`` → ``predefined/<id>.json``, with a complete
  ``pbtb`` block (name, title, title_zh, exchange, description, strategies) and
  our legacy top-level markers (``strategy_name``, ``strategies``, ``name``,
  ``description``) dropped, so the metadata lives in exactly one place.
  passivbot's own keys are untouched.
* ``site/templates/<old>.json`` → ``<id>.json``, carrying the new id, titles and
  the ``source_sha`` of the rewritten config, so the backtests are not re-run,
  and ``site/templates/index.json`` rebuilt from the result.

Usage::

    python scripts/rename_predefined.py                  # dry run
    python scripts/rename_predefined.py --apply --profile dev

Re-running after a completed migration is a no-op: a template whose old key is
already gone and whose new key is in place is skipped.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import subprocess
import sys
from pathlib import Path

from backtest_templates import write_index, write_json

REPO_ROOT = Path(__file__).resolve().parents[1]
SITE_DIR = REPO_ROOT / "site" / "templates"
BUCKET = "scalable-cluster-dev-bot-configs"
PREFIX = "predefined/"

# Our own markers, superseded by the `pbtb` block.
LEGACY_KEYS = ("strategy_name", "strategies", "name", "description")

# old name -> (id, english title, chinese title). Frozen: see the docstring.
CATALOG: dict[str, tuple[str, str, str]] = {
    # --- v7.12.0 line ------------------------------------------------------
    "bybit-cap100-iter1-winner-v712": (
        "bybit-mix3-100u-steady-v7",
        "3-coin basket · Steady · $100",
        "三币组合 · 稳健 · $100",
    ),
    "xrp-hft-r8x-241201251020": (
        "bybit-xrp-100u-bold-v7",
        "XRP only · Bold · $100",
        "XRP 单币 · 进取 · $100",
    ),
    "xrp-241201251009-r46x-lq": (
        "bybit-xrp-100u-extreme-v7-a",
        "XRP only · Extreme · $100 (A)",
        "XRP 单币 · 极限 · $100（A）",
    ),
    "xrp-r130x-241201251020": (
        "bybit-xrp-100u-extreme-v7-b",
        "XRP only · Extreme · $100 (B)",
        "XRP 单币 · 极限 · $100（B）",
    ),
    "bybit-cap300-iter1-winner-v712": (
        "bybit-mix8-300u-bold-v7",
        "8-coin basket · Bold · $300",
        "八币组合 · 进取 · $300",
    ),
    "bybit-cap500-iter3-lowdd-v712": (
        "bybit-mix10-500u-guard-v7",
        "10-coin basket · Guarded · $500",
        "十币组合 · 极保守 · $500",
    ),
    "bybit-cap500-iter1-hyst05-v712": (
        "bybit-mix10-500u-steady-v7",
        "10-coin basket · Steady · $500",
        "十币组合 · 稳健 · $500",
    ),
    "bybit-cap500-iter6-maxreturn-v712": (
        "bybit-mix10-500u-balanced-v7",
        "10-coin basket · Balanced · $500",
        "十币组合 · 平衡 · $500",
    ),
    "bybit-cap500-iter5-winner-v712": (
        "bybit-mix10-500u-bold-v7-a",
        "10-coin basket · Bold · $500 (A)",
        "十币组合 · 进取 · $500（A）",
    ),
    "bybit-cap500-iter6-highreturn-v712": (
        "bybit-mix10-500u-bold-v7-b",
        "10-coin basket · Bold · $500 (B)",
        "十币组合 · 进取 · $500（B）",
    ),
    # Drawdown 0.345 here and 0.359 on the v8 twin below; the pair takes the
    # tier the wider of the two falls in.
    "bybit-cap700-iter5-winner-v712": (
        "bybit-mix10-700u-bold-v7-a",
        "10-coin basket · Bold · $700 (A)",
        "十币组合 · 进取 · $700（A）",
    ),
    "bybit-cap700-iter2-winner-v712": (
        "bybit-mix10-700u-bold-v7-b",
        "10-coin basket · Bold · $700 (B)",
        "十币组合 · 进取 · $700（B）",
    ),
    "bybit-cap700-iter3-winner-v712": (
        "bybit-mix10-700u-bold-v7-c",
        "10-coin basket · Bold · $700 (C)",
        "十币组合 · 进取 · $700（C）",
    ),
    "bybit-cap700-iter2-maxreturn-v712": (
        "bybit-mix10-700u-bold-v7-d",
        "10-coin basket · Bold · $700 (D)",
        "十币组合 · 进取 · $700（D）",
    ),
    "bybit-cap1000-iter1-hyst05-v712": (
        "bybit-mix10-1000u-steady-v7",
        "10-coin basket · Steady · $1k",
        "十币组合 · 稳健 · $1k",
    ),
    "bybit-cap1000-iter2-balanced-v712": (
        "bybit-mix10-1000u-balanced-v7-a",
        "10-coin basket · Balanced · $1k (A)",
        "十币组合 · 平衡 · $1k（A）",
    ),
    "bybit-cap1000-iter4-winner-v712": (
        "bybit-mix10-1000u-balanced-v7-b",
        "10-coin basket · Balanced · $1k (B)",
        "十币组合 · 平衡 · $1k（B）",
    ),
    "bybit-cap1000-iter3-winner-v712": (
        "bybit-mix10-1000u-balanced-v7-c",
        "10-coin basket · Balanced · $1k (C)",
        "十币组合 · 平衡 · $1k（C）",
    ),
    "bybit-cap1000-iter2-maxreturn-v712": (
        "bybit-mix10-1000u-bold-v7",
        "10-coin basket · Bold · $1k",
        "十币组合 · 进取 · $1k",
    ),
    "xrp241201250401": (
        "bybit-xrp-10000u-steady-v7",
        "XRP only · Steady · $10k",
        "XRP 单币 · 稳健 · $10k",
    ),
    "xrp-r80-20241201251020": (
        "bybit-xrp-10000u-bold-v7",
        "XRP only · Bold · $10k",
        "XRP 单币 · 进取 · $10k",
    ),
    "xrp-cus": (
        "bybit-xrp-10000u-extreme-v7",
        "XRP only · Extreme · $10k",
        "XRP 单币 · 极限 · $10k",
    ),
    # --- v8.1.0 line -------------------------------------------------------
    "bybit-cap100-iter1-winner-v810": (
        "bybit-mix3-100u-steady-v8",
        "3-coin basket · Steady · $100",
        "三币组合 · 稳健 · $100",
    ),
    "bybit-xrp-hft-r8x-241201251020-v810": (
        "bybit-xrp-100u-bold-v8",
        "XRP only · Bold · $100",
        "XRP 单币 · 进取 · $100",
    ),
    "bybit-xrp-241201251009-r46x-lq-v810": (
        "bybit-xrp-100u-extreme-v8",
        "XRP only · Extreme · $100",
        "XRP 单币 · 极限 · $100",
    ),
    "bybit-cap300-iter1-winner-v810": (
        "bybit-mix8-300u-bold-v8",
        "8-coin basket · Bold · $300",
        "八币组合 · 进取 · $300",
    ),
    "bybit-cap700-iter5-winner-v810": (
        "bybit-mix10-700u-bold-v8",
        "10-coin basket · Bold · $700",
        "十币组合 · 进取 · $700",
    ),
    "bybit-cap1000-iter7-winner-v810": (
        "bybit-mix10-1000u-balanced-v8",
        "10-coin basket · Balanced · $1k",
        "十币组合 · 平衡 · $1k",
    ),
    "bybit-cap1000-iter7-highreturn-v810": (
        "bybit-mix10-1000u-bold-v8",
        "10-coin basket · Bold · $1k",
        "十币组合 · 进取 · $1k",
    ),
}


def aws(args: list[str], profile: str | None, capture: bool = True) -> str:
    cmd = ["aws", *args]
    if profile:
        cmd += ["--profile", profile]
    result = subprocess.run(cmd, check=True, capture_output=capture, text=False)
    return result.stdout.decode("utf-8") if capture else ""


def rewrite_config(raw: dict, new_id: str, title: str, title_zh: str) -> dict:
    """Return `raw` with one `pbtb` block and no legacy top-level markers."""
    old_meta = raw.get("pbtb") or {}
    sides = [
        s.get("side", "long")
        for s in (old_meta.get("strategies") or raw.get("strategies") or [{"side": "long"}])
    ]
    exchange = old_meta.get("exchange") or next(
        iter((raw.get("backtest") or {}).get("exchanges") or []), None
    )
    description = old_meta.get("description") or raw.get("description")

    out = {k: v for k, v in raw.items() if k not in LEGACY_KEYS and k != "pbtb"}
    # Anything else already under `pbtb` — an operator's `min_vip_level`, say —
    # belongs to the template, not to this rename; carry it through untouched.
    out["pbtb"] = {
        **old_meta,
        "name": new_id,
        "title": title,
        "title_zh": title_zh,
        "exchange": exchange,
        "description": description,
        "strategies": [{"name": new_id, "side": side} for side in sides],
    }
    return out


def body_bytes(config: dict) -> bytes:
    return json.dumps(config, indent=4, ensure_ascii=False).encode("utf-8")


def migrate_s3(profile: str | None, apply: bool, write: bool = True) -> dict[str, bytes]:
    """Rewrite every catalogued template under its new key. Returns id -> body.

    With `write` false the bodies are still derived — the artifacts key their
    `source_sha` off them — but nothing in the bucket is touched.
    """
    listing = aws(["s3", "ls", f"s3://{BUCKET}/{PREFIX}"], profile)
    present = {line.split()[-1] for line in listing.splitlines() if line.strip()}

    bodies: dict[str, bytes] = {}
    for old, (new_id, title, title_zh) in CATALOG.items():
        source = old if f"{old}.json" in present else new_id
        if source == new_id and f"{new_id}.json" not in present:
            print(f"  [miss] neither {old} nor {new_id} is in the bucket")
            continue
        raw = json.loads(aws(["s3", "cp", f"s3://{BUCKET}/{PREFIX}{source}.json", "-"], profile))
        body = body_bytes(rewrite_config(raw, new_id, title, title_zh))
        bodies[new_id] = body
        print(f"  {source:36} -> {new_id:32} {len(body):>7} bytes")
        if not (apply and write):
            continue
        tmp = REPO_ROOT / ".cache" / "rename_predefined" / f"{new_id}.json"
        tmp.parent.mkdir(parents=True, exist_ok=True)
        tmp.write_bytes(body)
        aws(
            ["s3", "cp", str(tmp), f"s3://{BUCKET}/{PREFIX}{new_id}.json",
             "--content-type", "application/json"],
            profile,
        )
        if source != new_id:
            aws(["s3", "rm", f"s3://{BUCKET}/{PREFIX}{old}.json"], profile)
    return bodies


def migrate_site(bodies: dict[str, bytes], apply: bool) -> None:
    """Rename the backtest artifacts and rebuild the index.

    The artifact's `source_sha` follows the config it was produced from; the
    rewrite changes those bytes without touching a single passivbot parameter,
    so the sha is carried over rather than the backtests re-run.
    """
    for old, (new_id, title, title_zh) in CATALOG.items():
        src = SITE_DIR / f"{old}.json"
        if not src.exists():
            src = SITE_DIR / f"{new_id}.json"
        if not src.exists():
            print(f"  [skip] no artifact for {old}")
            continue
        art = json.loads(src.read_text(encoding="utf-8"))
        art["name"] = new_id
        art["title"] = title
        art["title_zh"] = title_zh
        art["strategies"] = [
            {"name": new_id, "side": s.get("side", "long")} for s in (art.get("strategies") or [])
        ]
        if new_id in bodies:
            art["source_sha"] = hashlib.sha256(bodies[new_id]).hexdigest()
        print(f"  {src.name:40} -> {new_id}.json")
        if apply:
            write_json(SITE_DIR / f"{new_id}.json", art)
            if src.name != f"{new_id}.json":
                src.unlink()
    if apply:
        write_index()


def main() -> int:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("--apply", action="store_true", help="write (default is a dry run)")
    parser.add_argument("--profile", default=None, help="AWS CLI profile")
    parser.add_argument("--artifacts-only", action="store_true",
                        help="leave the bucket alone; rewrite site/templates only")
    args = parser.parse_args()

    ids = [v[0] for v in CATALOG.values()]
    if len(set(ids)) != len(ids):
        print("error: the catalog maps two templates onto one id", file=sys.stderr)
        return 1

    print(f"S3 {PREFIX} ({len(CATALOG)} templates):")
    bodies = migrate_s3(args.profile, args.apply, write=not args.artifacts_only)
    print("site/templates:")
    migrate_site(bodies, args.apply)
    if not args.apply:
        print("\n(dry run) re-run with --apply to write.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
