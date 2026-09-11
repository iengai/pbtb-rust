#!/usr/bin/env python3
"""Transfer a raw passivbot config into our S3 `predefined/` strategy store.

A raw passivbot optimizer/strategy config (e.g. the files under
`E:/projects/passivbot/configs`) is almost ready to use as one of our
predefined strategies. The ONLY thing our platform adds on top of the stock
passivbot schema is one top-level `pbtb` object:

  * `name` (string) — the template's id, and its S3 key: `tpl-` and eight
    random characters, generated unless --name reuses an existing one. Fixed
    for the life of the template: config-switch history rows and a bot's stored
    config quote it.
  * `universe`, `capital_usdt`, `style`, `profile`, `generation`, `engine` —
    the naming properties (scripts/template_naming.py). `style` and `engine`
    are read off the config; the rest come from the flags.
  * `title` / `title_zh` (string) — what a reader is shown the template as,
    composed from universe, profile and capital unless --title is given. The
    suffix that tells two identical titles apart is added by
    scripts/annotate_templates.py, which sees the whole catalogue.
  * `exchange` (string) — whose market data the strategy was tuned on.
  * `strategies` (array of {name, side}) — every side this strategy drives. A
    single-direction strategy lists one entry; a dual-sided one lists both
    `long` and `short`. A combined bot ends up with one entry per side, possibly
    from different strategies, but a single predefined file only describes its
    own strategy.
  * `description` (string, optional) — what a user reads about the template.
    Only written when --description is given; scripts/describe_templates.py
    composes the published one once the template has a backtest.

With --lab it also writes a top-level `lab` object beside `pbtb`: the strategy
lab's record of the tuning (source config, run, member, seeds, genome, notes).
No surface reads it and a bot built from the template does not copy it; see
scripts/annotate_templates.py for the fields.

Everything else (`bot`, `live`, `approved_coins`, `forced_mode_*`, leverage,
`coin_overrides`, ...) is left exactly as passivbot produced it. Per-bot tweaks
(`live.user`, `live.forced_mode_<side>`, risk/leverage) are applied later by the
telebot, NOT here. See docs/config-transfer.md for the full archive.

Usage:
  # Preview what would be written (no upload):
  python scripts/transfer_config_to_s3.py --config E:/projects/passivbot/configs/xrp-cus.json

  # Dual-sided strategy (default), upload to predefined/<new id>.json:
  python scripts/transfer_config_to_s3.py --config <raw.json> \
      --universe xrp --capital 100 --risk-profile steady --upload --profile dev

  # Single-direction strategy:
  python scripts/transfer_config_to_s3.py --config <raw.json> --sides long --upload --profile dev

  # With a strategy explanation:
  python scripts/transfer_config_to_s3.py --config <raw.json> --description "XRP grid, low leverage" --upload --profile dev

A new id is generated; --name overwrites an existing template instead.
"""

import argparse
import json
import os
import subprocess
import sys
import tempfile

from template_naming import FACETS, base_titles, engine_of, new_id, style_of

DEFAULT_BUCKET = "scalable-cluster-dev-bot-configs"
DEFAULT_PREFIX = "predefined/"
VALID_SIDES = ("long", "short")


def existing_template(target: str, profile: str | None) -> dict:
    """The object at `target`, or {} when there is none."""
    cmd = ["aws", "s3", "cp", target, "-"] + (["--profile", profile] if profile else [])
    result = subprocess.run(cmd, capture_output=True)
    return json.loads(result.stdout.decode("utf-8")) if result.returncode == 0 else {}


def transform(
    raw: dict,
    name: str,
    sides: list[str],
    description: str | None = None,
    title: str | None = None,
    title_zh: str | None = None,
    exchange: str | None = None,
    lab: dict | None = None,
    facets: dict | None = None,
) -> dict:
    """Return a copy of `raw` with our `pbtb` block (and `lab`, if given) injected.

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

    named = {k: v for k, v in (facets or {}).items() if v is not None}
    named.setdefault("style", style_of(raw))
    named.setdefault("engine", engine_of(raw))
    composed = base_titles(named) or (None, None)

    out = dict(raw)
    out["pbtb"] = {
        "name": name,
        "title": title or composed[0] or name,
        "title_zh": title_zh or composed[1] or title or name,
        **{k: named[k] for k in FACETS if k in named},
        "exchange": exchange
        or next(iter((raw.get("backtest") or {}).get("exchanges") or []), None),
        "description": description,
        "strategies": [{"name": name, "side": side} for side in sides],
    }
    if lab is not None:
        if not isinstance(lab, dict):
            raise ValueError("--lab must hold a JSON object")
        out["lab"] = lab
    return out


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--config", required=True, help="path to the raw passivbot config json")
    parser.add_argument("--name", default=None,
                        help="an existing template id to overwrite (default: a new tpl- id)")
    parser.add_argument("--universe", default=None, help="coin basket: mix3, mix8, mix10, xrp")
    parser.add_argument("--capital", type=int, default=None, help="balance tuned at, in USDT")
    parser.add_argument("--risk-profile", dest="risk_profile", default=None,
                        help="guard, steady, balanced, bold or extreme")
    parser.add_argument("--generation", type=int, default=None, help="the lab iteration")
    parser.add_argument("--title", default=None, help="override the composed title")
    parser.add_argument("--title-zh", dest="title_zh", default=None,
                        help="override the composed Chinese title")
    parser.add_argument("--exchange", default=None,
                        help="market data the strategy was tuned on (default: the backtest's)")
    parser.add_argument("--sides", default="long,short",
                        help="comma-separated sides this strategy drives (default: long,short)")
    parser.add_argument("--description", default=None,
                        help="free-text strategy explanation, stored as `pbtb.description`")
    parser.add_argument("--lab", default=None,
                        help="path to a JSON object stored verbatim as the internal `lab` block")
    parser.add_argument("--bucket", default=DEFAULT_BUCKET)
    parser.add_argument("--prefix", default=DEFAULT_PREFIX)
    parser.add_argument("--profile", default=None, help="AWS CLI profile for the upload")
    parser.add_argument("--upload", action="store_true",
                        help="actually upload (default is a dry-run preview)")
    parser.add_argument("--out", default=None,
                        help="also write the transformed config to this local path")
    args = parser.parse_args()

    name = args.name or new_id()
    sides = [s.strip() for s in args.sides.split(",") if s.strip()]
    # Overwriting keeps what the flags leave unsaid: the naming properties, the
    # level gate, the description, the lab record.
    existing = (existing_template(f"s3://{args.bucket}/{args.prefix}{name}.json", args.profile)
                if args.name else {})
    before = existing.get("pbtb") or {}

    with open(args.config, "r", encoding="utf-8") as fh:
        raw = json.load(fh)

    lab = None
    if args.lab:
        with open(args.lab, "r", encoding="utf-8") as fh:
            lab = json.load(fh)

    try:
        flags = {"universe": args.universe, "capital_usdt": args.capital,
                 "profile": args.risk_profile, "generation": args.generation}
        facets = {k: v if v is not None else before.get(k) for k, v in flags.items()}
        result = transform(
            raw, name, sides, args.description or before.get("description"), args.title,
            args.title_zh, args.exchange, lab if lab is not None else existing.get("lab"), facets,
        )
        result["pbtb"] = {**before, **result["pbtb"]}
    except ValueError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 1

    body = json.dumps(result, indent=4, ensure_ascii=False)
    key = f"{args.prefix}{name}.json"
    target = f"s3://{args.bucket}/{key}"

    meta = result["pbtb"]
    print(f"id          = {meta['name']}")
    print(f"title       = {meta['title']}  /  {meta['title_zh']}")
    print(f"properties  = {json.dumps({k: meta[k] for k in FACETS if k in meta}, ensure_ascii=False)}")
    print(f"strategies  = {json.dumps(meta['strategies'], ensure_ascii=False)}")
    print(f"description = {meta.get('description') or '—'}")
    print(f"target      = {target}")

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
