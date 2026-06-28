#!/usr/bin/env python3
"""Transfer a raw passivbot config into our S3 `predefined/` strategy store.

A raw passivbot optimizer/strategy config (e.g. the files under
`E:/projects/passivbot/configs`) is almost ready to use as one of our
predefined strategies. The ONLY things our platform adds on top of the stock
passivbot schema are these top-level marker properties:

  * `strategy_name` (string) — the strategy's stem, shown in the Telegram State
    view and used to attribute a bot's strategy.
  * `strategies` (array of {name, side}) — every side this strategy drives. A
    single-direction strategy lists one entry; a dual-sided one lists both
    `long` and `short`. A combined bot ends up with one entry per side, possibly
    from different strategies, but a single predefined file only describes its
    own strategy.
  * `description` (string, optional) — a free-text strategy explanation, shown
    in the Telegram State view. Only written when --description is given.

Everything else (`bot`, `live`, `approved_coins`, `forced_mode_*`, leverage,
`coin_overrides`, ...) is left exactly as passivbot produced it. Per-bot tweaks
(`live.user`, `live.forced_mode_<side>`, risk/leverage) are applied later by the
telebot, NOT here. See docs/config-transfer.md for the full archive.

Usage:
  # Preview what would be written (no upload):
  python scripts/transfer_config_to_s3.py --config E:/projects/passivbot/configs/xrp-cus.json

  # Dual-sided strategy (default), upload to predefined/<name>.json:
  python scripts/transfer_config_to_s3.py --config <raw.json> --upload --profile dev

  # Single-direction strategy:
  python scripts/transfer_config_to_s3.py --config <raw.json> --sides long --upload --profile dev

  # With a strategy explanation:
  python scripts/transfer_config_to_s3.py --config <raw.json> --description "XRP grid, low leverage" --upload --profile dev

The strategy name defaults to the input file's stem; override with --name.
"""

import argparse
import json
import os
import subprocess
import sys
import tempfile

DEFAULT_BUCKET = "scalable-cluster-dev-bot-configs"
DEFAULT_PREFIX = "predefined/"
VALID_SIDES = ("long", "short")


def transform(raw: dict, name: str, sides: list[str], description: str | None = None) -> dict:
    """Return a copy of `raw` with our marker properties injected.

    Pure function — this is the documented transfer contract. It does not mutate
    the input and touches nothing else in the config.
    """
    if not isinstance(raw, dict) or "bot" not in raw or "live" not in raw:
        raise ValueError(
            "input does not look like a passivbot config (missing top-level 'bot'/'live')"
        )
    for side in sides:
        if side not in VALID_SIDES:
            raise ValueError(f"invalid side {side!r}; expected one of {VALID_SIDES}")

    out = dict(raw)
    out["strategy_name"] = name
    out["strategies"] = [{"name": name, "side": side} for side in sides]
    if description:
        out["description"] = description
    return out


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--config", required=True, help="path to the raw passivbot config json")
    parser.add_argument("--name", default=None,
                        help="strategy name (default: input file stem)")
    parser.add_argument("--sides", default="long,short",
                        help="comma-separated sides this strategy drives (default: long,short)")
    parser.add_argument("--description", default=None,
                        help="free-text strategy explanation, stored as top-level `description`")
    parser.add_argument("--bucket", default=DEFAULT_BUCKET)
    parser.add_argument("--prefix", default=DEFAULT_PREFIX)
    parser.add_argument("--profile", default=None, help="AWS CLI profile for the upload")
    parser.add_argument("--upload", action="store_true",
                        help="actually upload (default is a dry-run preview)")
    parser.add_argument("--out", default=None,
                        help="also write the transformed config to this local path")
    args = parser.parse_args()

    name = args.name or os.path.splitext(os.path.basename(args.config))[0]
    sides = [s.strip() for s in args.sides.split(",") if s.strip()]

    with open(args.config, "r", encoding="utf-8") as fh:
        raw = json.load(fh)

    try:
        result = transform(raw, name, sides, args.description)
    except ValueError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 1

    body = json.dumps(result, indent=4, ensure_ascii=False)
    key = f"{args.prefix}{name}.json"
    target = f"s3://{args.bucket}/{key}"

    print(f"strategy_name = {result['strategy_name']}")
    print(f"strategies    = {json.dumps(result['strategies'])}")
    print(f"description   = {result.get('description', '—')}")
    print(f"target        = {target}")

    if args.out:
        with open(args.out, "w", encoding="utf-8") as fh:
            fh.write(body)
        print(f"wrote local copy: {args.out}")

    if not args.upload:
        print("\n(dry-run) re-run with --upload to push to S3.")
        return 0

    # Upload via the AWS CLI so we reuse the configured profile/credentials and
    # don't take a boto3 dependency.
    with tempfile.NamedTemporaryFile("w", suffix=".json", delete=False, encoding="utf-8") as tmp:
        tmp.write(body)
        tmp_path = tmp.name
    try:
        cmd = ["aws", "s3", "cp", tmp_path, target, "--content-type", "application/json"]
        if args.profile:
            cmd += ["--profile", args.profile]
        print(f"\n$ {' '.join(cmd)}")
        subprocess.run(cmd, check=True)
        print(f"uploaded {target}")
    finally:
        os.unlink(tmp_path)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
