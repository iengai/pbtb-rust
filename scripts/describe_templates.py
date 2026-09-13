#!/usr/bin/env python3
"""Compose each listed template's public description.

``pbtb.description`` is what a user reads about a template: the Telegram
confirm and State views, and the console's template page beside the backtest
chart. It is composed here rather than written by hand, so its numbers are the
ones that chart shows::

    <basket><direction><style>，<character>。建议本金 <capital> 起。
    📈 回测 <start>～<end>：<gain> 倍 · 📉 最大回撤 <drawdown>
    🧪 压力测试（单独起跑）· <a window the strategy lab ran>：<result>
    🎚 默认钱包敞口约 <exposure> 倍（杠杆倍数只是保证金档位）
    回测结果不代表未来收益。

The 📈 line is the site backtest (``site/templates/<id>.json``); a backtest
that did not complete its window is a liquidation and is said so, with no
multiple, as the site does. Basket, style and capital come from ``pbtb``. The
direction and exposure come from the config: the sides passivbot trades, one
with an exposure limit and positions to hold, whatever ``pbtb.strategies``
declares. The character and the 🧪 windows come from PUBLIC. The 🧪 windows are
the lab's separate runs over one stretch each (a crash, a bear leg, or the
months since a crash), started on their own. They differ from the site
backtest over the same dates, which arrives there carrying whatever it gained
or lost before.

A template with no PUBLIC entry or no backtest artifact is left as it is. Bot
configs built from a template are restamped with its description.

Run it after ``backtest_templates.py``, or after adding a template to PUBLIC::

    python scripts/describe_templates.py                  # dry run
    python scripts/describe_templates.py --apply --profile dev
"""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import textwrap
from pathlib import Path

from annotate_templates import ordered, refresh_artifact, restamp_bots, trading
from backtest_templates import write_index
from rename_predefined import ARCHIVED_PREFIX, BUCKET, PREFIX, aws, body_bytes
from template_naming import UNIVERSES, capital_label, exposure, traded_sides

REPO_ROOT = Path(__file__).resolve().parents[1]
SITE_DIR = REPO_ROOT / "site" / "templates"
CACHE = REPO_ROOT / ".cache" / "describe_templates"

STYLES_ZH = {"grid": "网格", "martingale": "马丁格尔", "ema_anchor": "EMA 锚定"}
SIDES_ZH = {("long",): "多头", ("short",): "空头", ("long", "short"): "双向"}
LIQUIDATED = "liquidated"

CRASH_2025 = "2025-10-10 崩盘"
BEAR_2026 = "2025-11-01～2026-03-01 熊市"
CRASH_2026 = "2026-06 崩盘"
SINCE_CRASH_2026 = "2026-06-06～09-04"


def window(label: str, drawdown: float | str, gain: float | None = None) -> dict:
    """A lab run on one window: its worst drawdown (or LIQUIDATED) and return."""
    return {"window": label, "drawdown": drawdown, "gain": gain}


PUBLIC: dict[str, dict] = {
    "tpl-2vewmtjy": {"character": "收益与回撤取中间档",
                     "stress": [window(CRASH_2025, 0.239),
                                window(BEAR_2026, 0.710, 0.055)]},
    "tpl-35c6wt6w": {"character": "收益优先",
                     "stress": [window(CRASH_2025, 0.243),
                                window(BEAR_2026, 0.758, -0.689)]},
    "tpl-3x8we339": {"character": "高敞口，止盈空间很薄、交易频繁，引擎版本不同结果差别很大",
                     "stress": [window(CRASH_2025, LIQUIDATED),
                                window(BEAR_2026, 0.325, 0.895),
                                window(SINCE_CRASH_2026, 0.116, 0.678)]},
    "tpl-5syk2duu": {"character": "盈利仓位多拿一段，收益优先",
                     "stress": [window(CRASH_2025, 0.289),
                                window(BEAR_2026, 0.684, -0.365),
                                window(CRASH_2026, 0.229, 0.297)]},
    "tpl-8brpubqf": {"character": "收益优先",
                     "stress": [window(CRASH_2025, 0.243),
                                window(BEAR_2026, 0.806, -0.383),
                                window(SINCE_CRASH_2026, 0.063, 0.158)]},
    "tpl-8bzdh8ay": {"character": "盈利仓位多拿一段，收益优先",
                     "stress": [window(CRASH_2025, 0.250),
                                window(BEAR_2026, 0.572, 0.012)]},
    "tpl-8ctkayqd": {"character": "在八个币之间轮动，敞口低",
                     "stress": [window(CRASH_2025, 0.348),
                                window(BEAR_2026, 0.793, -0.534)]},
    "tpl-bzwt9jn2": {"character": "盈利仓位多拿一段，收益与回撤取中间档",
                     "stress": [window(CRASH_2025, 0.248),
                                window(BEAR_2026, 0.490, 0.184),
                                window(CRASH_2026, 0.200, 0.262)]},
    "tpl-fhhjk83e": {"character": "敞口在 XRP 模板里最低，偏保守",
                     "stress": [window(BEAR_2026, 0.492, 0.195)]},
    "tpl-hfpuyzcm": {"character": "高敞口，扛不住急跌，只适合模拟盘或小额试跑",
                     "stress": [window(CRASH_2025, LIQUIDATED),
                                window(BEAR_2026, LIQUIDATED),
                                window(SINCE_CRASH_2026, 0.053, 0.115)]},
    "tpl-jwzxkxkh": {"character": "首仓占比大、加仓倍数低（浅网格）",
                     "stress": [window(BEAR_2026, 0.916, 0.544)]},
    "tpl-kypvfxgd": {"character": "自定义网格止盈，最多同时持 3 仓",
                     "stress": [window(BEAR_2026, LIQUIDATED)]},
    "tpl-m5xse3az": {"character": "在八个币之间轮动，敞口低",
                     "stress": [window(CRASH_2025, 0.348),
                                window(BEAR_2026, 0.790, -0.534),
                                window("2026-06-28～09-04", 0.072, 0.110)]},
    "tpl-mvgw3zk4": {"character": "几乎不换币，回撤控制优先；横盘时回本可能要 226 天",
                     "stress": [window(CRASH_2025, 0.014),
                                window(BEAR_2026, 0.035, -0.012)]},
    "tpl-nkh4sfw4": {"character": "高敞口，扛不住急跌，只适合模拟盘或小额试跑",
                     "stress": [window(CRASH_2025, LIQUIDATED),
                                window(BEAR_2026, 0.332, 0.805)]},
    "tpl-rwqvrc6u": {"character": "高敞口、标准加仓",
                     "stress": [window(BEAR_2026, LIQUIDATED)]},
    "tpl-san8qrvj": {"character": "为小资金调校，回撤低",
                     "stress": [window(CRASH_2025, 0.150),
                                window(BEAR_2026, 0.085, 0.026),
                                window(SINCE_CRASH_2026, 0.020, 0.023)]},
    "tpl-sappt9w2": {"character": "高敞口，以移动止盈为主",
                     "stress": [window(BEAR_2026, LIQUIDATED)]},
    "tpl-tavc364d": {"character": "换币不频繁",
                     "stress": [window(CRASH_2025, 0.202),
                                window(BEAR_2026, 0.505, 0.204)]},
    "tpl-xhdfc2ws": {"character": "换币不频繁",
                     "stress": [window(CRASH_2025, 0.155),
                                window(BEAR_2026, 0.667, 0.229),
                                window("2026-04-25～09-04", 0.369)]},
}


def number(value: float) -> str:
    return f"{value:.2f}".rstrip("0").rstrip(".")


def multiple(gain: float) -> str:
    return f"{gain:.1f}" if gain >= 10 else f"{gain:.2f}"


def basket(meta: dict, coins: list[str]) -> str:
    name = UNIVERSES[meta["universe"]][1]
    if len(coins) <= 1:
        return name
    listed = "、".join(coins) if len(coins) <= 3 else f"{coins[0]}、{coins[1]} 等 {len(coins)} 个币"
    return f"{name}（{listed}）"


def describe(config: dict, artifact: dict, entry: dict) -> str:
    meta = config["pbtb"]
    sides = traded_sides(config)
    lines = [
        f"{basket(meta, artifact.get('coins') or [])}{SIDES_ZH[sides]}"
        f"{STYLES_ZH.get(meta.get('style'), '')}，{entry['character']}。"
        f"建议本金 {capital_label(int(meta['capital_usdt']))} 起。"
    ]

    metrics = artifact["metrics"]
    span = f"{artifact['start']}～{artifact['end']}"
    completion = metrics.get("backtest_completion_ratio")
    if completion is not None and completion < 1:
        lines.append(f"📈 回测 {span}：跑到区间的 {completion:.1%} 时爆仓")
    else:
        lines.append(f"📈 回测 {span}：{multiple(metrics['gain'])} 倍"
                     f" · 📉 最大回撤 {metrics['drawdown_worst']:.1%}")

    for run in entry.get("stress", []):
        if run["drawdown"] == LIQUIDATED:
            result = "爆仓"
        else:
            result = f"回撤 {run['drawdown']:.1%}"
            if run["gain"] is not None:
                result = f"{run['gain']:+.1%} · {result}"
        lines.append(f"🧪 压力测试（单独起跑）· {run['window']}：{result}")

    if sides == ("long", "short"):
        held = (f"：多头约 {number(exposure(config, 'long'))} 倍"
                f" / 空头约 {number(exposure(config, 'short'))} 倍")
    else:
        held = f"约 {number(exposure(config, sides[0]))} 倍"
    lines.append(f"🎚 默认钱包敞口{held}（杠杆倍数只是保证金档位）")
    lines.append("回测结果不代表未来收益。")
    return "\n".join(lines)


def main() -> int:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("--apply", action="store_true", help="write (default is a dry run)")
    parser.add_argument("--profile", default=None, help="AWS CLI profile")
    args = parser.parse_args()

    metas: dict[str, dict] = {}
    listed: set[str] = set()
    artifacts = False
    for prefix in (PREFIX, ARCHIVED_PREFIX):
        listing = aws(["s3", "ls", f"s3://{BUCKET}/{prefix}"], args.profile)
        keys = sorted(line.split()[-1] for line in listing.splitlines()
                      if line.strip().endswith(".json"))
        print(f"{prefix} ({len(keys)}):")
        for key in keys:
            tid = key.removesuffix(".json")
            text = aws(["s3", "cp", f"s3://{BUCKET}/{prefix}{key}", "-"], args.profile)
            raw = json.loads(text)
            metas[tid] = raw.get("pbtb") or {}
            if prefix != PREFIX:
                continue
            listed.add(tid)
            entry = PUBLIC.get(tid)
            path = SITE_DIR / key
            if entry is None or not path.exists() or not traded_sides(raw):
                reason = ("no PUBLIC entry" if entry is None
                          else "no backtest artifact" if not path.exists() else "trades no side")
                print(f"  {tid}  left as it is: {reason}")
                continue

            artifact = json.loads(path.read_text(encoding="utf-8"))
            meta = ordered({**raw["pbtb"], "description": describe(raw, artifact, entry)})
            out = {**raw, "pbtb": meta}
            assert trading(out) == trading(raw), f"{tid}: would change what passivbot reads"
            metas[tid] = meta
            old, new = text.encode("utf-8"), body_bytes(out)
            if new == old:
                print(f"  {tid}  (current)")
                continue
            print(f"  {tid}  {meta.get('title_zh')}")
            print(textwrap.indent(meta["description"], "      "))
            artifacts |= refresh_artifact(tid, old, new, meta, args.apply)
            if not args.apply:
                continue
            tmp = CACHE / key
            tmp.parent.mkdir(parents=True, exist_ok=True)
            tmp.write_bytes(new)
            aws(["s3", "cp", str(tmp), f"s3://{BUCKET}/{prefix}{key}",
                 "--content-type", "application/json"], args.profile)

    unknown = sorted(set(PUBLIC) - listed)
    if unknown:
        print(f"PUBLIC names no listed template: {', '.join(unknown)}")
    if artifacts and args.apply:
        write_index()
        print("site/templates: source_sha refreshed, index rebuilt")
    restamp_bots(metas, args.profile, args.apply)
    if not args.apply:
        print("\n(dry run) re-run with --apply to write.")
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except subprocess.CalledProcessError as exc:
        print(f"error: {' '.join(exc.cmd[:4])} failed: {exc.stderr!r}", file=sys.stderr)
        raise SystemExit(1)
