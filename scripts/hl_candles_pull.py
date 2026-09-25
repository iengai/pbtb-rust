#!/usr/bin/env python3
"""Pull the stored Hyperliquid 1m candles into passivbot shards for the lab.

The hl-candle-collector Lambda stores one object per coin and UTC day,
``hyperliquid/1m/<COIN>/<YYYY-MM-DD>.json`` in the market-data bucket, holding
Hyperliquid's ``candleSnapshot`` rows as served. This mirrors the bucket under
``caches/hl_candles_raw`` in the passivbot checkout and writes each day as a
passivbot shard, 1440 rows of ``[t, o, h, l, c, v]`` float64, to
``caches/ohlcv_hl_1m/hyperliquid/1m/<COIN>/`` and ``<COIN>_USDC_USDC/`` (the
loader tries the coin name first), the layout ``caches/ohlcv_hl`` uses. A
minute without trades is absent from Hyperliquid's rows; it is written as the
previous close with zero volume.

A backtest on these runs with ``ohlcv_source_dir = "caches/ohlcv_hl_1m"`` and
``candle_interval_minutes = 1`` over days every coin has. Days missing inside a
coin's span, and days stored empty (no candle of the coin at all), are listed
and get no shard: Hyperliquid no longer serves them, so they stay gaps.

    python scripts/hl_candles_pull.py --pb-v8 E:/projects/passivbot --profile dev
"""

from __future__ import annotations

import argparse
import json
import struct
import subprocess
import sys
from datetime import date, timedelta
from pathlib import Path

from backtest_templates import DEFAULT_PB_V8

BUCKET = "scalable-cluster-dev-market-data"
PREFIX = "hyperliquid/1m"
RAW_DIR = "caches/hl_candles_raw"
SHARD_DIR = "caches/ohlcv_hl_1m"
MINUTE_MS = 60_000
DAY_MINUTES = 1440


def npy_bytes(rows: list[list[float]]) -> bytes:
    """``rows`` as a little-endian float64 ``.npy`` (format 1.0), no numpy needed."""
    header = f"{{'descr': '<f8', 'fortran_order': False, 'shape': ({len(rows)}, 6), }}"
    pad = 64 - (10 + len(header) + 1) % 64
    header = header + " " * (pad % 64) + "\n"
    flat = [v for row in rows for v in row]
    return b"\x93NUMPY\x01\x00" + struct.pack("<H", len(header)) + header.encode("latin1") + struct.pack(f"<{len(flat)}d", *flat)


def day_rows(day: str, candles: list[dict], prev_close: float | None) -> list[list[float]]:
    """The 1440 minutes of ``day``, each a served candle or the close before it."""
    start = _day_start_ms(day)
    by_minute = {int(c["t"]): c for c in candles}
    close = prev_close if prev_close is not None else float(candles[0]["o"])
    rows = []
    for i in range(DAY_MINUTES):
        t = start + i * MINUTE_MS
        c = by_minute.get(t)
        if c is None:
            rows.append([float(t), close, close, close, close, 0.0])
        else:
            close = float(c["c"])
            rows.append([float(t), float(c["o"]), float(c["h"]), float(c["l"]), close, float(c["v"])])
    return rows


def _day_start_ms(day: str) -> int:
    return (date.fromisoformat(day) - date(1970, 1, 1)).days * 86_400_000


def sync(raw: Path, profile: str | None, bucket: str) -> None:
    cmd = ["aws", "s3", "sync", f"s3://{bucket}/{PREFIX}", str(raw / PREFIX), "--only-show-errors"]
    if profile:
        cmd += ["--profile", profile]
    subprocess.run(cmd, check=True)


def gaps(days: list[str]) -> list[str]:
    held = set(days)
    first, last = date.fromisoformat(days[0]), date.fromisoformat(days[-1])
    return [
        (first + timedelta(n)).isoformat()
        for n in range((last - first).days + 1)
        if (first + timedelta(n)).isoformat() not in held
    ]


def write_coin(raw_coin: Path, shards: Path) -> tuple[int, list[str]]:
    """Write the coin's missing shards; the number written and the gap days."""
    coin = raw_coin.name
    days = sorted(p.stem for p in raw_coin.glob("*.json"))
    if not days:
        return 0, []
    targets = [shards / coin, shards / f"{coin}_USDC_USDC"]
    for t in targets:
        t.mkdir(parents=True, exist_ok=True)
    written, prev_close, prev_day, empty = 0, None, None, []
    for day in days:
        # The close before a day carries only across consecutive days.
        if prev_day is not None and date.fromisoformat(day) - date.fromisoformat(prev_day) != timedelta(1):
            prev_close = None
        candles = json.loads((raw_coin / f"{day}.json").read_text(encoding="utf-8"))
        if not candles:
            # Stored as [] when Hyperliquid had no candle of the coin all day.
            empty.append(day)
            prev_close, prev_day = None, day
            continue
        rows = day_rows(day, candles, prev_close)
        prev_close, prev_day = rows[-1][4], day
        if all((t / f"{day}.npy").exists() for t in targets):
            continue
        data = npy_bytes(rows)
        for t in targets:
            (t / f"{day}.npy").write_bytes(data)
        written += 1
    return written, sorted(gaps(days) + empty)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--pb-v8", type=Path, default=DEFAULT_PB_V8, help="the passivbot checkout the shards go into")
    parser.add_argument("--profile", default=None, help="AWS CLI profile for the S3 sync")
    parser.add_argument("--bucket", default=BUCKET)
    parser.add_argument("--no-sync", action="store_true", help="convert what is already mirrored")
    args = parser.parse_args()

    raw = args.pb_v8 / RAW_DIR
    if not args.no_sync:
        sync(raw, args.profile, args.bucket)
    shards = args.pb_v8 / SHARD_DIR / "hyperliquid" / "1m"
    coins = sorted(p for p in (raw / PREFIX).glob("*") if p.is_dir())
    if not coins:
        print(f"error: no candles under {raw / PREFIX}", file=sys.stderr)
        return 1
    for raw_coin in coins:
        written, missing = write_coin(raw_coin, shards)
        days = sorted(p.stem for p in raw_coin.glob("*.json"))
        line = f"{raw_coin.name}: {days[0]}..{days[-1]}, {written} new shard(s)" if days else f"{raw_coin.name}: empty"
        if missing:
            line += f", gaps: {', '.join(missing)}"
        print(line)
    return 0


if __name__ == "__main__":
    sys.exit(main())
