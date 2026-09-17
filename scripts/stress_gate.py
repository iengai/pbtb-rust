#!/usr/bin/env python3
"""The cold-start gate a config passes before it becomes a template.

``capital_usdt`` tells a reader the least capital the template is offered for,
and the reader starts today, with that much. The site's full-window backtest
says little about that: it starts in 2024, the account multiplies, and by the
bad stretches the starting balance no longer matters (96e827a3e2 reads
29-30x / dd 0.34 at both $500 and $700). Started fresh with the capital just
before a bad stretch, the same parameters part ways: FEB26 dd 0.319 at $700,
0.615 at $500.

So each window here is run on its own, from ``starting_balance = capital_usdt``,
and must complete (no liquidation) inside its drawdown cap. The windows and
caps are the strategy lab's eligibility gates (passivbot
``strategy_lab/scripts/harvest_round16.py``); a lab round that changes its
gates changes WINDOWS.

    python scripts/stress_gate.py --config <config.json> --capital 500

``transfer_config_to_s3.py`` runs it before every upload.
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import time
from dataclasses import dataclass
from pathlib import Path

import backtest_templates as bt


@dataclass(frozen=True)
class Window:
    label: str
    start: str
    end: str
    # The worst drawdown a fresh account may reach inside the window.
    drawdown_cap: float


WINDOWS = (
    Window("CRASH", "2025-10-01", "2025-10-20", 0.45),
    Window("BEAR", "2025-11-01", "2026-03-01", 0.50),
    # The bear leg from a week earlier: a fresh start's result swings with its day.
    Window("BEAR_m7", "2025-10-25", "2026-03-01", 0.50),
    Window("FEB26", "2026-01-10", "2026-02-20", 0.45),
    Window("LIVE26b", "2026-07-15", "2026-09-11", 0.35),
)
COMPLETE = 0.999


def window_config(config: dict, window: Window, capital: int) -> dict:
    """`config` set to run `window` alone, from `capital`."""
    out = json.loads(json.dumps(config))
    out.setdefault("backtest", {}).update(
        start_date=window.start, end_date=window.end, starting_balance=capital)
    return out


def failure(window: Window, metrics: dict) -> str | None:
    """Why a run of `window` does not pass, or None when it does."""
    completion = metrics.get("backtest_completion_ratio")
    drawdown = metrics.get("drawdown_worst")
    if completion is None or drawdown is None:
        return "the run reported no completion ratio or drawdown"
    if completion < COMPLETE:
        return f"liquidated {completion:.0%} into the window"
    if drawdown > window.drawdown_cap:
        return f"drawdown {drawdown:.1%} is past the {window.drawdown_cap:.0%} cap"
    return None


def run(config: dict, name: str, capital: int, pb_dirs: dict[str, Path], cache_dir: Path,
        timeout: float, log=print) -> dict:
    """Run every window; the record `lab.stress` keeps, `passed` among its fields."""
    templates = cache_dir / "templates"
    templates.mkdir(parents=True, exist_ok=True)
    windows = {}
    for window in WINDOWS:
        # Short: passivbot nests its output under the name, and Windows caps a path at 260.
        path = templates / f"{name}-{window.label}.json"
        path.write_text(json.dumps(window_config(config, window, capital), ensure_ascii=False),
                        encoding="utf-8")
        template = bt.Template(path, end_date=window.end)
        result_dir, _ = bt.run_backtest(template, pb_dirs[template.engine], cache_dir, timeout)
        analysis = json.loads((result_dir / "analysis.json").read_text(encoding="utf-8"))
        metrics = bt.pick_metrics(analysis)
        why = failure(window, metrics)
        gain, drawdown = metrics.get("gain"), metrics.get("drawdown_worst")
        engine, source = bt.ENGINE_VERSION[template.engine], template.backtest.get("ohlcv_source_dir")
        windows[window.label] = {
            "start": window.start, "end": window.end, "drawdown_cap": window.drawdown_cap,
            # The window's return as a fraction, the unit PUBLIC's windows take.
            "return": None if gain is None else gain - 1,
            "drawdown_worst": drawdown,
            "completion": metrics.get("backtest_completion_ratio"), "failure": why,
        }
        shown = (f"{gain - 1:+.1%} / dd {drawdown:.1%}"
                 if gain is not None and drawdown is not None else "no result")
        log(f"  {window.label:8} {window.start} -> {window.end}  {shown}  "
            f"{'ok' if why is None else 'FAIL: ' + why}")
    return {
        "capital": capital,
        "params_sha": bt.params_sha(config),
        "engine": engine,
        "ohlcv_source_dir": source,
        "passed": all(w["failure"] is None for w in windows.values()),
        "checked_at": int(time.time()),
        "windows": windows,
    }


def covers(record: dict | None, config: dict, capital: int) -> bool:
    """Whether `record` is a passed run of these parameters at this capital, on
    this engine and candle directory, over today's windows, so the run need
    not be repeated."""
    if not record or not record.get("passed"):
        return False
    same_windows = {
        label: (w.get("start"), w.get("end"), w.get("drawdown_cap"))
        for label, w in (record.get("windows") or {}).items()
    } == {w.label: (w.start, w.end, w.drawdown_cap) for w in WINDOWS}
    return (same_windows and record.get("capital") == capital
            and record.get("params_sha") == bt.params_sha(config)
            and record.get("engine") == bt.ENGINE_VERSION[bt.detect_engine(config)]
            and record.get("ohlcv_source_dir") == (config.get("backtest") or {}).get("ohlcv_source_dir"))


def add_engine_args(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--pb-v8", type=Path, default=bt.DEFAULT_PB_V8, help="passivbot v8.1.0 checkout")
    parser.add_argument("--pb-v7", type=Path, default=bt.DEFAULT_PB_V7, help="passivbot v7.12.0 checkout")
    parser.add_argument("--stress-cache-dir", type=Path, default=bt.REPO_ROOT / ".cache" / "stress_gate",
                        help="configs, logs and raw output of the stress runs")
    parser.add_argument("--stress-timeout", type=float, default=3600, help="seconds allowed per window")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--config", required=True, help="path to the passivbot config json")
    parser.add_argument("--capital", type=int, required=True, help="the capital to start each window from")
    add_engine_args(parser)
    args = parser.parse_args()
    config = json.loads(Path(args.config).read_text(encoding="utf-8"))
    try:
        record = run(config, Path(args.config).stem, args.capital,
                     {"v8": args.pb_v8, "v7": args.pb_v7}, args.stress_cache_dir, args.stress_timeout)
    except (RuntimeError, subprocess.TimeoutExpired) as exc:
        print(f"error: cold-start gate did not run: {exc}", file=sys.stderr)
        return 1
    print("passed" if record["passed"] else "FAILED")
    return 0 if record["passed"] else 1


if __name__ == "__main__":
    sys.exit(main())
