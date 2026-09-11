#!/usr/bin/env python3
"""Move every template from its readable id to an opaque one.

``bybit-mix10-1000u-bold-v8`` spelled out what the template was, so it went
stale whenever something about the template turned out otherwise: the 2026
crash window moved one template a whole profile, and every retirement left a
sibling letter that distinguished nothing. From here a template is addressed by
``tpl-<8 characters>`` and described by the naming properties in ``pbtb`` (see
template_naming.py).

One pass, idempotent, dry run by default:

* S3: each object in ``predefined/`` and ``retired/`` moves to its new key, with
  ``pbtb.name`` and its strategies on the new id, universe, capital and profile
  parsed from the readable id, the rest of the properties and the titles from
  annotate_templates.py, and ``lab.readable_id`` keeping the old id. Nothing
  passivbot reads changes.
* site/templates: each artifact renamed and carrying the new id, titles and
  properties, and the index rebuilt; ``source_sha`` follows the new bytes, so no
  backtest re-runs.
* Bots: each stored config's ``pbtb`` restamped onto the new id, titles and
  properties.
* Config-switch rows: ``template_name`` resolved to the new id, so a chart
  marker can be named by the template's title. The collector reads the rows
  afresh on its next run.

Usage::

    python scripts/opaque_ids.py                  # dry run
    python scripts/opaque_ids.py --apply --profile dev
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import subprocess
import sys
from pathlib import Path

from annotate_templates import LAB_KEY, curate, restamp_bots, trading
from backtest_templates import write_index, write_json
from rename_predefined import BUCKET, PREFIX, RETIRED_PREFIX, TABLE, aws, body_bytes
from template_naming import IDS, resolve, suffix

REPO_ROOT = Path(__file__).resolve().parents[1]
SITE_DIR = REPO_ROOT / "site" / "templates"
CACHE = REPO_ROOT / ".cache" / "opaque_ids"

READABLE = re.compile(
    r"bybit-(xrp|mix\d+)-(\d+)u-(guard|steady|balanced|bold|extreme)-v[78](?:-[a-z])?"
)
# Published as steady on a 0.154 drawdown; the 2026 crash window measured 36.9%.
PROFILE = {"bybit-mix10-1000u-steady-v7": "bold"}


def sha(body: bytes) -> str:
    return hashlib.sha256(body).hexdigest()


def moved(raw: dict, readable: str) -> dict:
    match = READABLE.fullmatch(readable)
    if not match:
        raise SystemExit(f"error: {readable} is not a readable template id")
    new_id = IDS[readable]
    meta = dict(raw.get("pbtb") or {})
    sides = [s.get("side", "long") for s in meta.get("strategies") or [{"side": "long"}]]
    meta.update(
        name=new_id,
        universe=match[1],
        capital_usdt=int(match[2]),
        profile=PROFILE.get(readable, match[3]),
        strategies=[{"name": new_id, "side": side} for side in sides],
    )
    lab = {**(raw.get(LAB_KEY) or {}), "readable_id": readable}
    return {**raw, "pbtb": meta, LAB_KEY: lab}


def move_artifact(stem: str, new_id: str, old: bytes, new: bytes, meta: dict,
                  apply: bool) -> bool:
    src = SITE_DIR / f"{stem}.json"
    if not src.exists():
        return False
    art = json.loads(src.read_text(encoding="utf-8"))
    if art.get("source_sha") == sha(old):
        art["source_sha"] = sha(new)
    else:
        print("    artifact was already stale; source_sha left for backtest_templates.py")
    art["name"] = new_id
    art["strategies"] = [
        {"name": new_id, "side": s.get("side", "long")} for s in art.get("strategies") or []
    ]
    for field in ("title", "title_zh", "style", "generation"):
        art[field] = meta.get(field)
    if apply:
        write_json(SITE_DIR / f"{new_id}.json", art)
        if stem != new_id:
            src.unlink()
    return True


def migrate_templates(profile: str | None, apply: bool) -> tuple[dict[str, dict], bool]:
    """Returns id -> `pbtb` for every template, and whether an artifact moved."""
    metas: dict[str, dict] = {}
    artifacts = False
    for prefix in (PREFIX, RETIRED_PREFIX):
        listing = aws(["s3", "ls", f"s3://{BUCKET}/{prefix}"], profile)
        keys = sorted(line.split()[-1] for line in listing.splitlines()
                      if line.strip().endswith(".json"))
        group: dict[str, dict] = {}
        before: dict[str, tuple[str, bytes, dict]] = {}
        for key in keys:
            stem = key.removesuffix(".json")
            text = aws(["s3", "cp", f"s3://{BUCKET}/{prefix}{key}", "-"], profile)
            raw = json.loads(text)
            readable = (raw.get(LAB_KEY) or {}).get("readable_id") or stem
            if readable not in IDS:
                raise SystemExit(f"error: {prefix}{key} has no opaque id")
            group[IDS[readable]] = moved(raw, readable)
            before[IDS[readable]] = (stem, text.encode("utf-8"), raw)

        print(f"{prefix} ({len(keys)}):")
        for new_id, body in sorted(curate(group).items(), key=lambda kv: before[kv[0]][0]):
            stem, old, raw = before[new_id]
            assert trading(body) == trading(raw), f"{stem}: would change what passivbot reads"
            new = body_bytes(body)
            meta = body["pbtb"]
            metas[new_id] = meta
            current = stem == new_id and new == old
            print(f"  {stem:34} -> {new_id}  {meta.get('title')}"
                  f"{'  (current)' if current else ''}")
            if current:
                continue
            if prefix == PREFIX:
                artifacts |= move_artifact(stem, new_id, old, new, meta, apply)
            if not apply:
                continue
            tmp = CACHE / f"{new_id}.json"
            tmp.parent.mkdir(parents=True, exist_ok=True)
            tmp.write_bytes(new)
            aws(["s3", "cp", str(tmp), f"s3://{BUCKET}/{prefix}{new_id}.json",
                 "--content-type", "application/json"], profile)
            if stem != new_id:
                aws(["s3", "rm", f"s3://{BUCKET}/{prefix}{stem}.json"], profile)
    return metas, artifacts


def restamp_switches(profile: str | None, apply: bool) -> None:
    print("config-switch rows:")
    scan = json.loads(aws([
        "dynamodb", "scan", "--table-name", TABLE,
        "--filter-expression", "begins_with(sk, :p)",
        "--expression-attribute-values", json.dumps({":p": {"S": "config_switch#"}}),
        "--output", "json",
    ], profile))
    for item in scan.get("Items", []):
        name = item.get("template_name", {}).get("S")
        new_id = resolve(name) if name else None
        if not name or new_id == name:
            continue
        print(f"  {item['sk']['S']:48} {name} -> {new_id}")
        if apply:
            aws([
                "dynamodb", "update-item", "--table-name", TABLE,
                "--key", json.dumps({"pk": item["pk"], "sk": item["sk"]}),
                "--update-expression", "SET template_name = :n",
                "--expression-attribute-values", json.dumps({":n": {"S": new_id}}),
            ], profile)


def main() -> int:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("--apply", action="store_true", help="write (default is a dry run)")
    parser.add_argument("--profile", default=None, help="AWS CLI profile")
    args = parser.parse_args()

    ids = list(IDS.values())
    if len(set(ids)) != len(ids) or len({suffix(i) for i in ids}) != len(ids):
        print("error: two templates share an id or its suffix", file=sys.stderr)
        return 1

    metas, artifacts = migrate_templates(args.profile, args.apply)
    if artifacts and args.apply:
        write_index()
        print("site/templates: artifacts moved, index rebuilt")
    restamp_bots(metas, args.profile, args.apply)
    restamp_switches(args.profile, args.apply)
    if args.apply:
        print("\ninvoke the daily-pnl collector so the chart markers quote the new ids.")
    else:
        print("\n(dry run) re-run with --apply to write.")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except subprocess.CalledProcessError as exc:
        print(f"error: {' '.join(exc.cmd[:4])} failed: {exc.stderr!r}", file=sys.stderr)
        raise SystemExit(1)
