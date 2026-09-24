#!/usr/bin/env python3
"""Build the Hyperliquid copy of a Bybit template, for the transfer to upload.

A template is for one exchange (docs/config-transfer.md, `exchange`). A tuning
the strategy lab built to run on both — every coin listed on both, Hyperliquid's
$10 order floor in every backtest, and a passed transfer check — is offered to
Hyperliquid bots as a second template: the same strategy, with a backtest on
Hyperliquid's candles.

Hyperliquid serves only its latest 5000 candles per interval, so there is no
1-minute history: the lab keeps its 1-hour candles as synthetic 1-minute rows
that aggregate back to them (passivbot ``strategy_lab/scripts/hl_check.py``),
under ``caches/ohlcv_hl`` in the passivbot checkout. The copy's backtest runs on
those at ``candle_interval_minutes`` 60, over the days every coin has, less
``WARMUP_DAYS`` at the start. Everything outside ``backtest`` is the Bybit
template's, fees included (above both exchanges' base tiers). ``pbtb`` is
dropped for the transfer to write; ``lab`` is kept, with the id it was copied
from.

    python scripts/hyperliquid_copy.py --from tpl-wjcxjbar --out hl.json --lab-out hl-lab.json
    python scripts/transfer_config_to_s3.py --config hl.json --lab hl-lab.json \\
        --exchange hyperliquid --universe mix24 --capital 10000 --risk-profile guard \\
        --generation 22 --sides long --pb-v8 E:/projects/passivbot --upload --profile dev
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
from datetime import date, timedelta
from pathlib import Path

from backtest_templates import DEFAULT_PB_V8

HL_SOURCE_DIR = "caches/ohlcv_hl"
CANDLE_MINUTES = 60
# Skipped at the start of the candles, so passivbot's warm-up reads real ones.
WARMUP_DAYS = 30
HL_MIN_COST = 10


def coins_of(config: dict) -> list[str]:
    approved = (config.get("live") or {}).get("approved_coins") or {}
    sides = approved.values() if isinstance(approved, dict) else [approved]
    return sorted({coin for side in sides for coin in side or []})


def shard_days(source: Path, coin: str) -> list[str]:
    """The days `source` holds a Hyperliquid 1m shard of `coin` for."""
    return sorted(p.stem for p in (source / "hyperliquid" / "1m" / f"{coin}_USDC_USDC").glob("*.npy"))


def window(days: dict[str, list[str]]) -> tuple[str, str]:
    """The backtest window every coin has candles for, less the warm-up."""
    missing = sorted(coin for coin, held in days.items() if not held)
    if missing:
        raise ValueError(f"no Hyperliquid candles for {', '.join(missing)}")
    first = max(held[0] for held in days.values())
    last = min(held[-1] for held in days.values())
    start = (date.fromisoformat(first) + timedelta(days=WARMUP_DAYS)).isoformat()
    if start >= last:
        raise ValueError(f"the candles run {first}..{last}, too short for a {WARMUP_DAYS}-day warm-up")
    return start, last


def hyperliquid_copy(template: dict, start: str, end: str) -> tuple[dict, dict]:
    """The copy's config (no `pbtb`) and its `lab` block."""
    meta = template.get("pbtb") or {}
    if meta.get("exchange", "bybit") != "bybit":
        raise ValueError(f"{meta.get('name')} is a {meta.get('exchange')} template, not a Bybit one")
    config = {k: v for k, v in template.items() if k not in ("pbtb", "lab")}
    backtest = dict(config.get("backtest") or {})
    overrides = dict((backtest.get("market_settings") or {}).get("overrides") or {})
    for coin in coins_of(config):
        overrides[coin] = {**overrides.get(coin, {}), "min_cost": max(
            HL_MIN_COST, (overrides.get(coin) or {}).get("min_cost") or 0)}
    backtest.update(
        exchanges=["hyperliquid"],
        ohlcv_source_dir=HL_SOURCE_DIR,
        candle_interval_minutes=CANDLE_MINUTES,
        start_date=start,
        end_date=end,
        market_settings={**(backtest.get("market_settings") or {}), "overrides": overrides},
    )
    config["backtest"] = backtest
    lab = {**(template.get("lab") or {}), "copied_from": meta.get("name")}
    return config, lab


def read_template(name: str, profile: str | None) -> dict:
    path = Path(name)
    if path.exists():
        return json.loads(path.read_text(encoding="utf-8"))
    cmd = ["aws", "s3", "cp", f"s3://scalable-cluster-dev-bot-configs/predefined/{name}.json", "-"]
    if profile:
        cmd += ["--profile", profile]
    return json.loads(subprocess.run(cmd, check=True, capture_output=True).stdout.decode("utf-8"))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--from", dest="source", required=True, help="the Bybit template: an id in S3, or a file")
    parser.add_argument("--out", required=True, help="where to write the copy's config")
    parser.add_argument("--lab-out", required=True, help="where to write its lab block, for the transfer's --lab")
    parser.add_argument("--pb-v8", type=Path, default=DEFAULT_PB_V8, help="the passivbot checkout holding the candles")
    parser.add_argument("--profile", default=None, help="AWS CLI profile for the S3 read")
    args = parser.parse_args()

    template = read_template(args.source, args.profile)
    source = args.pb_v8 / HL_SOURCE_DIR
    try:
        start, end = window({coin: shard_days(source, coin) for coin in coins_of(template)})
        config, lab = hyperliquid_copy(template, start, end)
    except ValueError as exc:
        print(f"error: {exc}", file=sys.stderr)
        return 1
    Path(args.out).write_text(json.dumps(config, indent=4, ensure_ascii=False), encoding="utf-8")
    Path(args.lab_out).write_text(json.dumps(lab, indent=4, ensure_ascii=False), encoding="utf-8")
    print(f"{lab['copied_from']} -> {args.out}: Hyperliquid 1h candles {start} -> {end}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
