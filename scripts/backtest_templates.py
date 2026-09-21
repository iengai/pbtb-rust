#!/usr/bin/env python3
"""Backtest every predefined passivbot template and emit compact site artifacts.

Templates live in S3 under ``predefined/``; each one is a complete passivbot
config with its ``backtest`` window already set plus a ``pbtb`` block carrying
the display metadata. The script runs each template through the passivbot
backtester that matches its engine line and writes, under ``site/templates``:

* ``index.json`` - one row per template with headline metrics, sorted by name
* ``<name>.json`` - the row plus a downsampled, index-normalized equity curve

and, after a run that synced the templates, the public catalogue's audience
overlay in the chart bucket (``AUDIENCES_URL``), which the site reads over
each row's ``audience``.

Usage::

    python scripts/backtest_templates.py [--only NAME ...] [--engine v7|v8]
        [--end-date DATE|now] [--ohlcv-source-dir DIR] [--force] [--no-sync]
        [--capital-profile] [--pb-v8 DIR] [--pb-v7 DIR] [--cache-dir DIR]

Each backtest runs as a subprocess inside its passivbot checkout with the
checkout's own virtualenv. A template is skipped when its artifact already
carries the same ``source_sha`` or ``trading_sha``, engine, window end and
candle directory, and (under ``--capital-profile``, or when the artifact has a
capital profile) a profile run on the same. ``--force`` reruns it, profile
included.

``--end-date`` runs every template from its own start to that date; ``now``
is the last day with a complete candle set, two days back, the date passivbot
itself resolves ``now`` to. Without it a template keeps the window end its
artifact was last run to, or runs to its own ``backtest.end_date`` when it
has no artifact yet, so a plain rerun after adding a template costs only the
new one and never moves the others. The template in S3 is not touched: the
date goes into the run's copy only, and into the artifact's ``end``.

``--capital-profile`` runs every selected template that has no current capital
profile, its own balance included, and then the same window at the balances of
``CAPITAL_LADDER`` above its own, and records, per balance, the
gain, the worst drawdown and the coins the fills went to. A template's capital
is the least it is offered for, not a promise that more behaves the same: a
small balance cannot place the first order on an expensive coin, and a config
that holds one position can take a different path at one balance. A profile is
kept while the engine, the strategy, the window end and the candle directory
it was run on stand; once a template has one, a rerun that finds it stale runs
it again without the flag, so a profile never sits beside metrics of another
run. A rung that fails leaves the template without a profile; its own run is
still written.

A run reads candles from its ``backtest.ohlcv_source_dir`` when one is set
(the mainstream templates name ``caches/ohlcv_padded``, whose alts end
2025-10-28 and BTC and XRP 2025-11-18; the XRP templates that set none read
passivbot's own data), and passivbot does not fail when the window runs past
the end of that directory: the run completes without the candles past its last
day, and ``backtest_completion_ratio`` still reports the whole window. When a
run has a candle directory and a window end, the script checks before running
that the directory holds a shard for every coin through the day before that
end, and fails the template when it does not; a run with no candle directory
is not checked. ``--ohlcv-source-dir`` points every run's copy at another
directory (relative to each passivbot checkout), which is how a window is run
past the end of ``caches/ohlcv_padded``; the artifact records the directory
its run read, so a rerun with a different one is not skipped.
"""

from __future__ import annotations

import argparse
import csv
import gzip
import hashlib
import json
import os
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
from datetime import datetime, timedelta, timezone
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[1]
S3_PREFIX = "s3://scalable-cluster-dev-bot-configs/predefined/"
# The public catalogue's audience overlay (src/domain/templateaudience.rs): the
# ids of the templates offered to everyone, which the site reads through the
# showcase CDN over the snapshot's own `audience`. Every script that rebuilds
# the snapshot from S3 rewrites it, so an overlay never outlives a newer one.
AUDIENCES_URL = "s3://scalable-cluster-dev-return-charts/public/templates/audience.json"
PUBLIC_CACHE_CONTROL = "public, max-age=30"
OUTPUT_DIR = REPO_ROOT / "site" / "templates"
DEFAULT_CACHE_DIR = REPO_ROOT / ".cache" / "backtest_templates"
# Sibling checkouts: the main passivbot tree is at v8.1.0; pb-v712 is a git
# worktree of the same repo pinned to v7.12.0 with its own venv and Rust build.
DEFAULT_PB_V8 = REPO_ROOT.parent / "passivbot"
DEFAULT_PB_V7 = REPO_ROOT.parent / "pb-v712"

ENGINE_VERSION = {"v8": "v8.1.0", "v7": "v7.12.0"}
# The balances a capital profile is run at: those above the template's own.
CAPITAL_LADDER = (300, 500, 700, 1000, 1500, 2000, 3000, 5000, 10000)
# How `fill_shares` derives a row's coins, in a profile's key: a committed
# profile was run on an engine, a strategy, a window and a candle directory,
# and read out this way. Bumping it makes every committed profile stale, so
# the next run recomputes rather than keeping rows the current code would
# not produce.
FILL_SHARES_REV = 2
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


def source_dir_gap(source_dir: Path, exchange: str, coins: list[str], end_date: str) -> str | None:
    """Why ``source_dir`` cannot back a run to ``end_date``, or None when every
    coin has a daily shard through the day before it. Shards are
    ``<source_dir>/<exchange>/1m/<COIN>_<quote>_<settle>/<YYYY-MM-DD>.npy``."""
    last_needed = (datetime.fromisoformat(end_date).date() - timedelta(days=1)).isoformat()
    for coin in coins:
        dirs = sorted((source_dir / exchange / "1m").glob(f"{coin}_*"))
        days = sorted(p.stem for d in dirs for p in d.glob("*.npy"))
        if not days:
            return f"{source_dir} holds no {exchange} 1m shards for {coin}"
        if days[-1] < last_needed:
            return f"{source_dir} ends {days[-1]} for {coin}, before the window end {end_date}"
    return None


def artifact_end(name: str) -> str | None:
    """The window end the committed artifact was run to; None without one."""
    try:
        return json.loads((OUTPUT_DIR / f"{name}.json").read_text(encoding="utf-8")).get("end")
    except (OSError, ValueError):
        return None


# The blocks of ours a template carries beside what passivbot reads.
OURS = ("pbtb", "lab")


def trading_sha(config: dict) -> str:
    """The sha of what passivbot reads, over a canonical encoding: blind to our
    blocks and to formatting, so a template whose audience the console switched
    (which rewrites the object) still matches the backtest run on it."""
    strategy = {k: v for k, v in config.items() if k not in OURS}
    body = json.dumps(strategy, sort_keys=True, ensure_ascii=False, separators=(",", ":"))
    return hashlib.sha256(body.encode("utf-8")).hexdigest()


def params_sha(config: dict) -> str:
    """The sha of what the bot trades by: the strategy without its ``backtest``
    block. The same tuning at another ``backtest.starting_balance`` carries
    the same one, which is how the transfer refuses a second template of it."""
    return trading_sha({k: v for k, v in config.items() if k != "backtest"})


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
        self.trading_sha = trading_sha(self.config)
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


def position_class(config: dict) -> str | None:
    """``single`` for a config that holds one position at a time on every side it
    trades, ``multi`` for one that holds several, ``None`` when it trades no side.
    A side holds what passivbot does: ``n_positions`` rounded, and no more than
    it has coins approved, so a one-coin template is ``single`` whatever
    ``n_positions`` says. ``multi`` never says how many: the artifacts are public
    and the count is a strategy parameter."""
    approved = (config.get("live") or {}).get("approved_coins")
    held = []
    for side in ("long", "short"):
        bot = (config.get("bot") or {}).get(side) or {}
        risk = bot["risk"] if isinstance(bot.get("risk"), dict) else bot
        if float(risk.get("total_wallet_exposure_limit") or 0) > 0:
            n = float(risk.get("n_positions", bot.get("n_positions")) or 0)
            coins = approved.get(side) if isinstance(approved, dict) else approved
            held.append(round(min(n, len(coins)) if isinstance(coins, list) and coins else n))
    held = [n for n in held if n > 0]
    if not held:
        return None
    return "multi" if max(held) >= 2 else "single"


def sync_templates(templates_dir: Path, profile: str | None, run=subprocess.run) -> None:
    templates_dir.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, AWS_PROFILE=profile) if profile else None
    cmd = ["aws", "s3", "sync", S3_PREFIX, str(templates_dir), "--exclude", "*", "--include", "*.json"]
    print(f"syncing {S3_PREFIX} -> {templates_dir}")
    run(cmd, check=True, env=env)


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


def candle_dir(template: Template, source_dir: str | None) -> str | None:
    """The candle directory a run reads: the override, else the template's own."""
    return source_dir or template.backtest.get("ohlcv_source_dir")


def artifact_is_current(template: Template, source_dir: str | None = None) -> bool:
    out = OUTPUT_DIR / f"{template.name}.json"
    if not out.exists():
        return False
    try:
        existing = json.loads(out.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return False
    return (
        (existing.get("source_sha") == template.source_sha
         or existing.get("trading_sha") == template.trading_sha)
        and existing.get("engine") == ENGINE_VERSION[template.engine]
        and existing.get("end") == template.end_date
        and existing.get("ohlcv_source_dir") == candle_dir(template, source_dir)
    )


def run_backtest(
    template: Template, pb_dir: Path, cache_dir: Path, timeout: float, source_dir: str | None = None,
    balance: float | None = None,
) -> tuple[Path, str]:
    """Run one template through passivbot; return the result directory and the attempt label that produced it.
    With `balance`, the run starts from it instead of the template's own and keeps its own run directory."""
    source = candle_dir(template, source_dir)
    if source and template.end_date:
        gap = source_dir_gap(pb_dir / source, template.exchange or "bybit", template.coins, template.end_date)
        if gap:
            raise RuntimeError(gap)

    run_dir = cache_dir / "runs" / (template.name if balance is None else f"{template.name}@{balance:g}")
    if run_dir.exists():
        shutil.rmtree(run_dir)
    run_dir.mkdir(parents=True)

    config = json.loads(template.raw.decode("utf-8"))
    config.setdefault("backtest", {})["base_dir"] = run_dir.as_posix()
    if template.end_date:
        config["backtest"]["end_date"] = template.end_date
    if source_dir:
        config["backtest"]["ohlcv_source_dir"] = source_dir
    if balance is not None:
        config["backtest"]["starting_balance"] = balance

    log_dir = cache_dir / "logs"
    log_dir.mkdir(parents=True, exist_ok=True)
    log_path = log_dir / f"{run_dir.name}.log"

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


def fill_shares(result_dir: Path) -> list[dict]:
    """The coins a run's fills went to, as a percentage of the fill count, largest first."""
    path = result_dir / "fills.csv"
    if not path.exists():
        return []
    with path.open(newline="", encoding="utf-8") as handle:
        coins = [row.get("coin") for row in csv.DictReader(handle)]
    coins = [coin for coin in coins if coin]
    if not coins:
        return []
    counts: dict[str, int] = {}
    for coin in coins:
        counts[coin] = counts.get(coin, 0) + 1
    # Every coin a fill went to, so the shares account for the whole run: a
    # reader counting the list against the template's coins is told which of
    # them the run never touched. Changing what this returns means bumping
    # FILL_SHARES_REV, or committed profiles keep the old derivation.
    shares = [{"coin": coin, "share": round(100 * n / len(coins), 1)} for coin, n in counts.items()]
    return sorted(shares, key=lambda s: (-s["share"], s["coin"]))


def profile_row(balance: float, result_dir: Path) -> dict:
    metrics = pick_metrics(json.loads((result_dir / "analysis.json").read_text(encoding="utf-8")))
    return {
        "balance": balance,
        "gain": metrics.get("gain"),
        "drawdown_worst": metrics.get("drawdown_worst"),
        "coins": fill_shares(result_dir),
    }


def ladder_for(balance: float | None) -> list[float]:
    """The template's own balance, then every rung of the ladder above it."""
    own = float(balance or 0)
    return [own] + [float(rung) for rung in CAPITAL_LADDER if rung > own]


def profile_key(template: Template, source_dir: str | None) -> str:
    """What a capital profile was run on: the engine, the strategy, the window end, the candle
    directory and the revision of how its rows are read out of a run."""
    ran_on = (ENGINE_VERSION[template.engine], template.trading_sha, template.end_date,
              candle_dir(template, source_dir), FILL_SHARES_REV)
    return ":".join(str(part) for part in ran_on)


def capital_profile(
    template: Template, own_result: Path, pb_dir: Path, cache_dir: Path, timeout: float, source_dir: str | None,
) -> dict:
    own, *above = ladder_for(template.backtest.get("starting_balance"))
    rows = [profile_row(own, own_result)]
    for balance in above:
        result_dir, _ = run_backtest(template, pb_dir, cache_dir, timeout, source_dir, balance=balance)
        rows.append(profile_row(balance, result_dir))
    return {"key": profile_key(template, source_dir), "rows": rows}


def artifact_profile(name: str) -> dict | None:
    """The capital profile on a template's artifact, current or not."""
    try:
        profile = json.loads((OUTPUT_DIR / f"{name}.json").read_text(encoding="utf-8")).get("capital_profile")
    except (OSError, ValueError):
        return None
    return profile if isinstance(profile, dict) else None


def kept_profile(template: Template, source_dir: str | None) -> dict | None:
    """The profile on the template's artifact, while it was run on what a run now would read."""
    profile = artifact_profile(template.name)
    return profile if profile and profile.get("key") == profile_key(template, source_dir) else None


def wants_profile(flag: bool, force: bool, had: bool, kept: bool) -> bool:
    """Whether a run profiles a template: asked for or already carrying one, and not current (or forced)."""
    return (flag or had) and (force or not kept)


def capital_drawdown(profile: dict | None) -> dict | None:
    """The median and the worst of a profile's drawdowns: what the list shows beside a template's own."""
    values = [row["drawdown_worst"] for row in (profile or {}).get("rows", []) if row.get("drawdown_worst") is not None]
    if not values:
        return None
    return {"median": statistics.median(values), "worst": max(values)}


def build_artifact(template: Template, result_dir: Path, source_dir: str | None = None) -> dict:
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
        # How many coins it holds at once, as a class: the catalogue's first split.
        "positions": position_class(template.config),
        "engine": ENGINE_VERSION[template.engine],
        # `"operator"` on a template offered to the operator's account only.
        "audience": template.pbtb.get("audience"),
        "exchange": template.exchange,
        "coins": template.coins,
        "start": template.backtest.get("start_date"),
        "end": template.end_date,
        "starting_balance": template.backtest.get("starting_balance"),
        # The candle directory the run read, relative to the passivbot checkout.
        "ohlcv_source_dir": candle_dir(template, source_dir),
        # No description: the artifacts are public and the authors' notes name
        # leverage, position counts and exposure caps. The description reaches
        # a signed-in user through the API instead.
        "strategies": template.strategies,
        "metrics": pick_metrics(analysis),
        # The coins the fills went to, which for a config that holds one
        # position are far fewer than the basket `coins` names.
        "traded": fill_shares(result_dir),
        "points": load_points(result_dir),
        "source_sha": template.source_sha,
        "trading_sha": template.trading_sha,
        "params_sha": params_sha(template.config),
        "generated_at": int(time.time()),
    }


def write_json(path: Path, data) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, ensure_ascii=False, separators=(",", ":")), encoding="utf-8")


def write_index() -> None:
    """Rebuild index.json from every per-template artifact on disk."""
    index_fields = (
        "name", "title", "title_zh", "style", "generation", "positions", "engine", "audience", "exchange",
        "coins", "start", "end", "starting_balance", "params_sha", "metrics", "traded",
    )
    rows = []
    for path in sorted(OUTPUT_DIR.glob("*.json")):
        if path.name == "index.json":
            continue
        artifact = json.loads(path.read_text(encoding="utf-8"))
        row = {field: artifact.get(field) for field in index_fields}
        # The profile itself stays in the template's own file; the list needs two numbers of it.
        row["capital_drawdown"] = capital_drawdown(artifact.get("capital_profile"))
        rows.append(row)
    rows.sort(key=lambda row: row["name"])
    write_json(OUTPUT_DIR / "index.json", rows)


def published_templates(templates_dir: Path) -> list[str]:
    """The ids a member is listed: every synced template not marked for the operator.

    The same rule as ``ConfigTemplate::is_operator_only``: only a ``pbtb.audience``
    of exactly ``"operator"`` retires a template.
    """
    names = []
    for path in sorted(templates_dir.glob("*.json")):
        if path.stat().st_size == 0:
            continue
        config = json.loads(path.read_text(encoding="utf-8"))
        meta = config.get("pbtb") if isinstance(config, dict) else None
        if not (isinstance(meta, dict) and meta.get("audience") == "operator"):
            names.append(path.stem)
    return names


def publish_template_audiences(templates_dir: Path, profile: str | None, run=subprocess.run) -> None:
    """Write the public catalogue's overlay from templates just synced from S3."""
    body = json.dumps(
        {"generated_at": int(time.time()), "published": published_templates(templates_dir)},
        separators=(",", ":"),
    )
    cmd = [
        "aws", "s3", "cp", "-", AUDIENCES_URL,
        "--content-type", "application/json", "--cache-control", PUBLIC_CACHE_CONTROL,
    ]
    if profile:
        cmd += ["--profile", profile]
    print(f"publishing the template audiences -> {AUDIENCES_URL}")
    run(cmd, input=body.encode("utf-8"), check=True)


def republish_template_audiences(profile: str | None, run=subprocess.run) -> None:
    """Sync ``predefined/`` into a scratch directory and publish the overlay from it.

    For a script that changed templates in S3 without a local mirror of them.
    """
    with tempfile.TemporaryDirectory() as scratch:
        sync_templates(Path(scratch), profile, run)
        publish_template_audiences(Path(scratch), profile, run)


def publish_if_synced(synced: bool, profile: str | None, run=subprocess.run) -> None:
    """Publish the overlay at the end of a run that synced the templates.

    From a fresh sync, not the run's own mirror: the backtests can take hours,
    so a switch made in the console meanwhile would be taken back, and the
    mirror keeps the file of a template archived or renamed since. A run on
    cached templates publishes nothing: the templates may be older than an
    audience the console set, and the run never said to read S3.
    """
    if synced:
        republish_template_audiences(profile, run)
    else:
        print("--no-sync: the template audiences were not published")


def parse_args(argv=None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--only", nargs="+", metavar="NAME", help="template names (file stems) to process")
    parser.add_argument("--engine", choices=("v7", "v8"), help="restrict to one engine line")
    parser.add_argument("--end-date", metavar="DATE", help="run every template to this date (YYYY-MM-DD, or `now`) instead of its own window end")
    parser.add_argument("--ohlcv-source-dir", metavar="DIR", help="candle directory every run reads instead of the template's own, relative to each passivbot checkout")
    parser.add_argument("--force", action="store_true", help="rerun templates whose artifact is current")
    parser.add_argument("--capital-profile", action="store_true",
                        help="also run each processed template at the larger balances of CAPITAL_LADDER")
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
        kept = kept_profile(template, args.ohlcv_source_dir)
        profiling = bool(template.backtest.get("starting_balance")) and wants_profile(
            args.capital_profile, args.force, artifact_profile(template.name) is not None, kept is not None
        )
        if not args.force and not profiling and artifact_is_current(template, args.ohlcv_source_dir):
            summary.append((template.name, engine, 0.0, "skipped"))
            print(f"[skip] {template.name} ({engine}) artifact is current")
            continue
        print(f"[run ] {template.name} ({engine}){' + capital profile' if profiling else ''} ...", flush=True)
        started = time.time()
        try:
            result_dir, attempt = run_backtest(
                template, pb_dirs[template.engine], cache_dir, args.timeout, args.ohlcv_source_dir
            )
            artifact = build_artifact(template, result_dir, args.ohlcv_source_dir)
            status = "ok" if attempt == "verbatim" else f"ok ({attempt})"
            # The own-balance row comes from the run above, so a profile costs the rungs over it alone.
            profile = None if profiling else kept
            if profiling:
                try:
                    profile = capital_profile(
                        template, result_dir, pb_dirs[template.engine], cache_dir, args.timeout, args.ohlcv_source_dir
                    )
                except Exception as exc:  # a failed rung costs the profile, not the template's own run
                    status = f"{status} (no capital profile: {exc})"
            if profile:
                artifact["capital_profile"] = profile
            write_json(OUTPUT_DIR / f"{template.name}.json", artifact)
        except Exception as exc:  # a failed template must not abort the run
            status = f"failed: {exc}"
        elapsed = time.time() - started
        summary.append((template.name, engine, elapsed, status))
        tag = "ok" if status.startswith("ok") else "FAIL"
        print(f"[{tag}] {template.name} {elapsed:.0f}s {status if status != 'ok' else ''}".rstrip(), flush=True)

    write_index()
    publish_if_synced(args.sync, args.profile)

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
