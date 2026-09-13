#!/usr/bin/env python3
"""Backtest every predefined passivbot template and emit compact site artifacts.

Templates live in S3 under ``predefined/``; each one is a complete passivbot
config with its ``backtest`` window already set plus a ``pbtb`` block carrying
the display metadata. The script runs each template through the passivbot
backtester that matches its engine line and writes, under ``site/templates``:

* ``index.json`` - one row per template with headline metrics, sorted by name
* ``<name>.json`` - the row plus a downsampled, index-normalized equity curve

Usage::

    python scripts/backtest_templates.py [--only NAME ...] [--engine v7|v8]
        [--end-date DATE|now] [--force] [--no-sync] [--pb-v8 DIR] [--pb-v7 DIR]
        [--cache-dir DIR]

Each backtest runs as a subprocess inside its passivbot checkout with the
checkout's own virtualenv. A template whose artifact already carries the same
``source_sha``, engine and window end is skipped unless ``--force`` is given.

``--end-date`` runs every template from its own start to that date; ``now``
is the last day with a complete candle set, two days back, the date passivbot
itself resolves ``now`` to. Without it a template keeps the window end its
artifact was last run to, or runs to its own ``backtest.end_date`` when it
has no artifact yet, so a plain rerun after adding a template costs only the
new one and never moves the others. The template in S3 is not touched: the
date goes into the run's copy only, and into the artifact's ``end``.
"""

from __future__ import annotations

import argparse
import csv
import gzip
import hashlib
import json
import os
import shutil
import subprocess
import sys
import time
from datetime import datetime, timedelta, timezone
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
S3_PREFIX = "s3://scalable-cluster-dev-bot-configs/predefined/"
OUTPUT_DIR = REPO_ROOT / "site" / "templates"
DEFAULT_CACHE_DIR = REPO_ROOT / ".cache" / "backtest_templates"
# Sibling checkouts: the main passivbot tree is at v8.1.0; pb-v712 is a git
# worktree of the same repo pinned to v7.12.0 with its own venv and Rust build.
DEFAULT_PB_V8 = REPO_ROOT.parent / "passivbot"
DEFAULT_PB_V7 = REPO_ROOT.parent / "pb-v712"

ENGINE_VERSION = {"v8": "v8.1.0", "v7": "v7.12.0"}
MAX_POINTS = 500

# Canonical metric name -> raw analysis.json keys, first present wins.
#
# Both engines emit most metrics twice, denominated in USD and in BTC, with a
# ``_usd`` / ``_btc`` suffix; the site shows USD so the ``_usd`` variant comes
# first. Keys whose value is identical in both denominations (position timing,
# loss/profit ratio, completion ratio) are written once without a suffix.
# Unsuffixed fallbacks cover configs analysed by an engine that predates the
# dual-denomination layout. Neither v7.12.0 nor v8.1.0 emits a per-side
# exposure metric (they report ``exposure_mean_ratio_usd`` for the whole
# account), so ``exposure_ratios_mean_long`` / ``_short`` resolve to null; the
# candidates are kept for engines that do emit them.
METRIC_KEYS: dict[str, tuple[str, ...]] = {
    "gain": ("gain_usd", "gain"),
    "adg": ("adg_usd", "adg"),
    "adg_w": ("adg_w_usd", "adg_w"),
    "drawdown_worst": ("drawdown_worst_usd", "drawdown_worst"),
    "drawdown_worst_mean_1pct": (
        "drawdown_worst_mean_1pct_usd",
        "drawdown_worst_mean_1pct",
    ),
    "sharpe_ratio": ("sharpe_ratio_usd", "sharpe_ratio"),
    "sortino_ratio": ("sortino_ratio_usd", "sortino_ratio"),
    "calmar_ratio": ("calmar_ratio_usd", "calmar_ratio"),
    "omega_ratio": ("omega_ratio_usd", "omega_ratio"),
    "sterling_ratio": ("sterling_ratio_usd", "sterling_ratio"),
    "loss_profit_ratio": ("loss_profit_ratio", "loss_profit_ratio_usd"),
    "positions_held_per_day": ("positions_held_per_day",),
    "position_held_hours_mean": ("position_held_hours_mean",),
    "position_unchanged_hours_max": ("position_unchanged_hours_max",),
    "equity_balance_diff_neg_max": (
        "equity_balance_diff_neg_max_usd",
        "equity_balance_diff_neg_max",
    ),
    "exposure_ratios_mean_long": (
        "exposure_ratios_mean_long",
        "exposure_ratios_mean_long_usd",
    ),
    "exposure_ratios_mean_short": (
        "exposure_ratios_mean_short",
        "exposure_ratios_mean_short_usd",
    ),
    # Fraction of the requested window the backtest actually covered; < 1
    # means the curve ends early (liquidation or missing candles).
    "backtest_completion_ratio": ("backtest_completion_ratio",),
}

# balance_and_equity.csv.gz column candidates, first present wins.
BALANCE_COLUMNS = ("usd_total_balance", "balance")
EQUITY_COLUMNS = ("usd_total_equity", "equity")


def artifact_end(name: str) -> str | None:
    """The window end the committed artifact was run to; None without one."""
    try:
        return json.loads((OUTPUT_DIR / f"{name}.json").read_text(encoding="utf-8")).get("end")
    except (OSError, ValueError):
        return None


class Template:
    """One template file plus the metadata the site needs from it.

    ``end_date`` is the window end the run uses: the one ``--end-date`` names
    for every template, else the end its artifact was last run to, else the
    template's own."""

    def __init__(self, path: Path, end_date: str | None = None):
        self.path = path
        self.name = path.stem
        self.raw = path.read_bytes()
        self.source_sha = hashlib.sha256(self.raw).hexdigest()
        self.config = json.loads(self.raw.decode("utf-8"))
        self.engine = detect_engine(self.config)
        self.end_date = end_date or artifact_end(self.name) or self.backtest.get("end_date")

    @property
    def backtest(self) -> dict:
        return self.config.get("backtest") or {}

    @property
    def pbtb(self) -> dict:
        return self.config.get("pbtb") or {}

    @property
    def exchange(self) -> str | None:
        if self.pbtb.get("exchange"):
            return self.pbtb["exchange"]
        exchanges = self.backtest.get("exchanges") or []
        if isinstance(exchanges, str):
            return exchanges
        return exchanges[0] if exchanges else self.backtest.get("exchange")

    @property
    def coins(self) -> list[str]:
        coins = self.backtest.get("coins")
        if isinstance(coins, dict) and self.exchange in coins and coins[self.exchange]:
            return list(coins[self.exchange])
        if isinstance(coins, list) and coins:
            return list(coins)
        approved = (self.config.get("live") or {}).get("approved_coins") or {}
        if isinstance(approved, dict):
            merged: list[str] = []
            for side in ("long", "short"):
                for coin in approved.get(side) or []:
                    if coin not in merged:
                        merged.append(coin)
            return merged
        if isinstance(approved, list):
            return list(approved)
        return []

    @property
    def description(self) -> str | None:
        return self.pbtb.get("description") or self.config.get("description")

    @property
    def strategies(self):
        if "strategies" in self.pbtb:
            return self.pbtb["strategies"]
        top = self.config.get("strategies")
        return top if isinstance(top, list) else None


def detect_engine(config: dict) -> str:
    """The engine line a config runs on: its ``config_version``'s major, or — on
    one carrying none — its shape, since v8 nests each side's exposure under
    ``risk``. The launcher routes a bot by the same rule."""
    version = str(config.get("config_version") or "")
    if version:
        return "v8" if version.startswith("v8") else "v7"
    bot = config.get("bot") or {}
    nested = any(isinstance((bot.get(side) or {}).get("risk"), dict) for side in ("long", "short"))
    return "v8" if nested else "v7"


def sync_templates(templates_dir: Path, profile: str) -> None:
    templates_dir.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, AWS_PROFILE=profile)
    cmd = ["aws", "s3", "sync", S3_PREFIX, str(templates_dir), "--exclude", "*", "--include", "*.json"]
    print(f"syncing {S3_PREFIX} -> {templates_dir}")
    subprocess.run(cmd, check=True, env=env)


def load_templates(templates_dir: Path, end_date: str | None = None) -> list[Template]:
    templates = []
    for path in sorted(templates_dir.glob("*.json")):
        # The prefix itself is mirrored as a zero-byte object; it is not a config.
        if path.stat().st_size == 0:
            continue
        templates.append(Template(path, end_date))
    return templates


def resolve_end_date(value: str | None) -> str | None:
    """``now`` as passivbot resolves it (two days back, so the last day's
    candles are complete), any other date as given, ``None`` untouched."""
    if value is None:
        return None
    if value == "now":
        day = datetime.now(timezone.utc).date() - timedelta(days=2)
        return day.isoformat()
    return datetime.fromisoformat(value).date().isoformat()


def venv_python(pb_dir: Path) -> Path:
    if os.name == "nt":
        return pb_dir / ".venv" / "Scripts" / "python.exe"
    return pb_dir / ".venv" / "bin" / "python"


def artifact_is_current(template: Template) -> bool:
    out = OUTPUT_DIR / f"{template.name}.json"
    if not out.exists():
        return False
    try:
        existing = json.loads(out.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return False
    return (
        existing.get("source_sha") == template.source_sha
        and existing.get("engine") == ENGINE_VERSION[template.engine]
        and existing.get("end") == template.end_date
    )


def run_backtest(template: Template, pb_dir: Path, cache_dir: Path, timeout: float) -> tuple[Path, str]:
    """Run one template through passivbot; return the result directory and the attempt label that produced it."""
    run_dir = cache_dir / "runs" / template.name
    if run_dir.exists():
        shutil.rmtree(run_dir)
    run_dir.mkdir(parents=True)

    config = json.loads(template.raw.decode("utf-8"))
    config.setdefault("backtest", {})["base_dir"] = run_dir.as_posix()
    if template.end_date:
        config["backtest"]["end_date"] = template.end_date

    log_dir = cache_dir / "logs"
    log_dir.mkdir(parents=True, exist_ok=True)
    log_path = log_dir / f"{template.name}.log"

    python = venv_python(pb_dir)
    if not python.exists():
        raise RuntimeError(f"no venv python at {python}")

    # The template is run verbatim first. The optimizer section plays no part
    # in a backtest on paper, yet on v7.12.0 the mere presence of
    # optimize.bounds in a pre-versioned config shifts the fills slightly
    # (bisected on xrp-r80: gain 73.33206 with bounds, 73.33207 without), so
    # it is only dropped as a retry when the engine rejects the config outright
    # (the oldest templates carry bounds naming parameters that were since
    # migrated away, e.g. close_grid_markup_range).
    attempts = [("verbatim", config)]
    if "optimize" in config:
        attempts.append(("optimize block dropped", {k: v for k, v in config.items() if k != "optimize"}))
    with log_path.open("wb") as log:
        for label, attempt in attempts:
            config_path = run_dir / "config.json"
            config_path.write_text(json.dumps(attempt, ensure_ascii=False, indent=2), encoding="utf-8")
            returncode = invoke_backtest(python, pb_dir, config_path, log, timeout)
            if returncode == 0:
                break
            log.write(f"\n=== attempt '{label}' exited {returncode}\n".encode("utf-8"))
        else:
            raise RuntimeError(f"backtest exited {returncode}; see {log_path}")

    results = sorted(run_dir.glob("*/*/analysis.json"), key=lambda p: p.stat().st_mtime)
    if not results:
        raise RuntimeError(f"no analysis.json under {run_dir}; see {log_path}")
    return results[-1].parent, label


def invoke_backtest(python: Path, pb_dir: Path, config_path: Path, log, timeout: float) -> int:
    # Several templates ship with backtest.suite_enabled on, which makes
    # passivbot run its is_full/is_early/is_late scenario suite under
    # suite_runs/ instead of one full-window backtest; --suite n wins over
    # the config on both engines.
    cmd = [
        str(python),
        "src/backtest.py",
        "--skip-rust-compile",
        str(config_path),
        "--disable_plotting",
        "--suite",
        "n",
    ]
    # passivbot prints config excerpts containing the Chinese descriptions;
    # forcing UTF-8 keeps that from tripping the console codepage on Windows.
    env = dict(os.environ, PYTHONIOENCODING="utf-8", PYTHONUTF8="1")
    log.write((" ".join(cmd) + "\n").encode("utf-8"))
    log.flush()
    proc = subprocess.run(
        cmd, cwd=str(pb_dir), stdout=log, stderr=subprocess.STDOUT, env=env, timeout=timeout
    )
    return proc.returncode


def pick_metrics(analysis: dict) -> dict:
    metrics = {}
    for canonical, candidates in METRIC_KEYS.items():
        value = None
        for key in candidates:
            if analysis.get(key) is not None:
                value = analysis[key]
                break
        metrics[canonical] = value
    return metrics


def parse_timestamp(value: str) -> int:
    """Row index of balance_and_equity.csv.gz as unix seconds (UTC)."""
    text = value.strip()
    try:
        number = float(text)
    except ValueError:
        stamp = datetime.fromisoformat(text)
        if stamp.tzinfo is None:
            stamp = stamp.replace(tzinfo=timezone.utc)
        return int(stamp.timestamp())
    # Numeric indices are millisecond timestamps.
    return int(number / 1000) if number > 1e11 else int(number)


def load_points(result_dir: Path) -> list[dict]:
    rows = []
    with gzip.open(result_dir / "balance_and_equity.csv.gz", "rt", encoding="utf-8") as fh:
        reader = csv.reader(fh)
        header = next(reader)
        bal_idx = next(header.index(c) for c in BALANCE_COLUMNS if c in header)
        eq_idx = next(header.index(c) for c in EQUITY_COLUMNS if c in header)
        for row in reader:
            rows.append((parse_timestamp(row[0]), float(row[bal_idx]), float(row[eq_idx])))
    if not rows:
        return []

    if len(rows) > MAX_POINTS:
        step = (len(rows) - 1) / (MAX_POINTS - 1)
        rows = [rows[round(i * step)] for i in range(MAX_POINTS)]

    base_balance = rows[0][1] or 1.0
    base_equity = rows[0][2] or 1.0
    return [
        {
            "ts": ts,
            "equity": round(equity / base_equity * 100, 4),
            "balance": round(balance / base_balance * 100, 4),
        }
        for ts, balance, equity in rows
    ]


def build_artifact(template: Template, result_dir: Path) -> dict:
    analysis = json.loads((result_dir / "analysis.json").read_text(encoding="utf-8"))
    return {
        "name": template.name,
        # What a reader is shown the template as, against the name that
        # addresses it. Absent on a template published before titles.
        "title": template.pbtb.get("title"),
        "title_zh": template.pbtb.get("title_zh"),
        # Naming properties a card shows as tags beside the title.
        "style": template.pbtb.get("style"),
        "generation": template.pbtb.get("generation"),
        "engine": ENGINE_VERSION[template.engine],
        "exchange": template.exchange,
        "coins": template.coins,
        "start": template.backtest.get("start_date"),
        "end": template.end_date,
        "starting_balance": template.backtest.get("starting_balance"),
        # No description: the artifacts are public and the authors' notes name
        # leverage, position counts and exposure caps. The description reaches
        # a signed-in user through the API instead.
        "strategies": template.strategies,
        "metrics": pick_metrics(analysis),
        "points": load_points(result_dir),
        "source_sha": template.source_sha,
        "generated_at": int(time.time()),
    }


def write_json(path: Path, data) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, ensure_ascii=False, separators=(",", ":")), encoding="utf-8")


def write_index() -> None:
    """Rebuild index.json from every per-template artifact on disk."""
    index_fields = (
        "name", "title", "title_zh", "style", "generation", "engine", "exchange", "coins",
        "start", "end", "metrics",
    )
    rows = []
    for path in sorted(OUTPUT_DIR.glob("*.json")):
        if path.name == "index.json":
            continue
        artifact = json.loads(path.read_text(encoding="utf-8"))
        rows.append({field: artifact.get(field) for field in index_fields})
    rows.sort(key=lambda row: row["name"])
    write_json(OUTPUT_DIR / "index.json", rows)


def parse_args(argv=None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--only", nargs="+", metavar="NAME", help="template names (file stems) to process")
    parser.add_argument("--engine", choices=("v7", "v8"), help="restrict to one engine line")
    parser.add_argument("--end-date", metavar="DATE", help="run every template to this date (YYYY-MM-DD, or `now`) instead of its own window end")
    parser.add_argument("--force", action="store_true", help="rerun templates whose artifact is current")
    parser.add_argument("--sync", dest="sync", action="store_true", default=True, help="sync templates from S3 (default)")
    parser.add_argument("--no-sync", dest="sync", action="store_false", help="use the cached templates as-is")
    parser.add_argument("--profile", default=os.environ.get("AWS_PROFILE", "dev"), help="AWS profile for the S3 sync")
    parser.add_argument("--pb-v8", type=Path, default=DEFAULT_PB_V8, help="passivbot v8.1.0 checkout")
    parser.add_argument("--pb-v7", type=Path, default=DEFAULT_PB_V7, help="passivbot v7.12.0 checkout")
    parser.add_argument("--cache-dir", type=Path, default=DEFAULT_CACHE_DIR, help="templates, logs and raw backtest output")
    parser.add_argument("--timeout", type=float, default=3 * 3600, help="seconds allowed per backtest")
    return parser.parse_args(argv)


def main(argv=None) -> int:
    args = parse_args(argv)
    cache_dir: Path = args.cache_dir.resolve()
    templates_dir = cache_dir / "templates"
    if args.sync:
        sync_templates(templates_dir, args.profile)

    templates = load_templates(templates_dir, resolve_end_date(args.end_date))
    if args.only:
        wanted = set(args.only)
        unknown = wanted - {t.name for t in templates}
        if unknown:
            print(f"unknown template(s): {', '.join(sorted(unknown))}", file=sys.stderr)
            return 2
        templates = [t for t in templates if t.name in wanted]
    if args.engine:
        templates = [t for t in templates if t.engine == args.engine]
    # v8 runs first: its checkout is the primary one and the runs are the cheapest to verify.
    templates.sort(key=lambda t: (t.engine != "v8", t.name))

    pb_dirs = {"v8": args.pb_v8.resolve(), "v7": args.pb_v7.resolve()}
    summary: list[tuple[str, str, float, str]] = []
    for template in templates:
        engine = ENGINE_VERSION[template.engine]
        if not args.force and artifact_is_current(template):
            summary.append((template.name, engine, 0.0, "skipped"))
            print(f"[skip] {template.name} ({engine}) artifact is current")
            continue
        print(f"[run ] {template.name} ({engine}) ...", flush=True)
        started = time.time()
        try:
            result_dir, attempt = run_backtest(template, pb_dirs[template.engine], cache_dir, args.timeout)
            artifact = build_artifact(template, result_dir)
            write_json(OUTPUT_DIR / f"{template.name}.json", artifact)
            status = "ok" if attempt == "verbatim" else f"ok ({attempt})"
        except Exception as exc:  # a failed template must not abort the run
            status = f"failed: {exc}"
        elapsed = time.time() - started
        summary.append((template.name, engine, elapsed, status))
        tag = "ok" if status.startswith("ok") else "FAIL"
        print(f"[{tag}] {template.name} {elapsed:.0f}s {status if status != 'ok' else ''}".rstrip(), flush=True)

    write_index()

    width = max((len(row[0]) for row in summary), default=4)
    print()
    print(f"{'name':<{width}}  {'engine':<8} {'seconds':>8}  status")
    for name, engine, seconds, status in summary:
        print(f"{name:<{width}}  {engine:<8} {seconds:>8.0f}  {status}")
    failed = sum(1 for row in summary if row[3].startswith("failed"))
    print(f"\n{len(summary)} template(s), {failed} failed; index at {OUTPUT_DIR / 'index.json'}")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
