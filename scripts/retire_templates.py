#!/usr/bin/env python3
"""Take a template out of the catalogue without destroying it.

A retired template moves from ``predefined/`` to ``retired/`` in the same
bucket. Nothing lists it after that — ``S3TemplateRepository::list`` scans the
``predefined/`` prefix only — so it leaves the Telegram chooser, the API and the
site, while the object, its version history and any bot that still names it are
untouched. Moving it back is the same command with the prefixes swapped.

Its backtest artifact is deleted from ``site/templates`` and the index rebuilt,
so the published catalogue drops it too. The artifact is recoverable from git,
and from the object, by re-running ``backtest_templates.py``.

Retiring a template a bot's stored config still names is refused: the bot would
keep running (nothing resolves a template by name at launch) but its config
could no longer be re-applied, and the refusal is cheaper than finding out
later.

Usage::

    python scripts/retire_templates.py <id> [<id> …]                 # dry run
    python scripts/retire_templates.py <id> … --apply --profile dev
    python scripts/retire_templates.py <id> … --restore --apply --profile dev
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from pathlib import Path

from backtest_templates import write_index
from template_naming import resolve

REPO_ROOT = Path(__file__).resolve().parents[1]
SITE_DIR = REPO_ROOT / "site" / "templates"
BUCKET = "scalable-cluster-dev-bot-configs"
LIVE = "predefined/"
RETIRED = "retired/"
TABLE = "scalable-cluster-dev-bots"


def aws(args: list[str], profile: str | None) -> str:
    cmd = ["aws", *args]
    if profile:
        cmd += ["--profile", profile]
    return subprocess.run(cmd, check=True, capture_output=True, text=False).stdout.decode("utf-8")


def templates_in_use(profile: str | None) -> dict[str, str]:
    """template id -> the bot whose stored config resolves to it."""
    scan = json.loads(
        aws(["dynamodb", "scan", "--table-name", TABLE, "--output", "json"], profile)
    )
    in_use: dict[str, str] = {}
    for item in scan.get("Items", []):
        pk = item.get("pk", {}).get("S", "")
        sk = item.get("sk", {}).get("S", "")
        if not pk.startswith("user_id#") or "#" in sk:
            continue  # only bot rows, whose sk is the bare bot_id
        user_id = pk.removeprefix("user_id#")
        key = f"{user_id}/{sk}/{sk}.json"
        try:
            body = json.loads(aws(["s3", "cp", f"s3://{BUCKET}/{key}", "-"], profile))
        except subprocess.CalledProcessError:
            continue  # a bot with no config yet
        meta = body.get("pbtb") or {}
        name = meta.get("name") or body.get("strategy_name") or body.get("name")
        # A config keeps the name it was applied under, which may predate the
        # current id; unresolved, the check would wave through a running template.
        if name:
            in_use[resolve(name)] = item.get("name", {}).get("S", sk)
    return in_use


def main() -> int:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("ids", nargs="+", help="template ids to retire")
    parser.add_argument("--apply", action="store_true", help="write (default is a dry run)")
    parser.add_argument("--profile", default=None, help="AWS CLI profile")
    parser.add_argument("--restore", action="store_true", help="move back into the catalogue")
    args = parser.parse_args()

    # A template named by an id it had before is acted on under its current one.
    ids = [resolve(i) for i in args.ids]
    src, dst = (RETIRED, LIVE) if args.restore else (LIVE, RETIRED)

    if not args.restore:
        in_use = templates_in_use(args.profile)
        blocked = [(i, in_use[i]) for i in ids if i in in_use]
        if blocked:
            for template, bot in blocked:
                print(f"error: {template} is the stored config of {bot!r}", file=sys.stderr)
            return 1

    for template in ids:
        print(f"  {src}{template}.json -> {dst}{template}.json")
        if not args.apply:
            continue
        aws(["s3", "mv", f"s3://{BUCKET}/{src}{template}.json",
             f"s3://{BUCKET}/{dst}{template}.json"], args.profile)
        artifact = SITE_DIR / f"{template}.json"
        if args.restore:
            print("    (re-run backtest_templates.py to publish its backtest again)")
        elif artifact.exists():
            artifact.unlink()
            print(f"    removed {artifact.relative_to(REPO_ROOT)}")

    if args.apply and not args.restore:
        write_index()
        print(f"  index.json rebuilt from {len(list(SITE_DIR.glob('*.json'))) - 1} artifacts")
    if args.apply:
        print("  re-run annotate_templates.py --apply: the title suffixes follow the catalogue")
    if not args.apply:
        print("\n(dry run) re-run with --apply to write.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
