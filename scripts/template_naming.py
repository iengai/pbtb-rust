#!/usr/bin/env python3
"""What a template is called, and by what it is addressed.

A template is addressed by an opaque id, ``tpl-`` and eight random characters.
It is the S3 key ``predefined/<id>.json``, the site URL and the Telegram
callback, and what a bot's stored config and its config-switch rows quote. It
says nothing about the template, so nothing learnt about the template later can
make it wrong, and it never changes.

What a reader is told lives in ``pbtb`` as properties, each read off the config
or the strategy lab:

    universe       mix3 | mix8 | mix10 | xrp          the coin basket
    capital_usdt   100 … 10000                         the balance it was tuned at
    style          grid | martingale | ema_anchor      the strategy family
    profile        guard | steady | balanced | bold | extreme
    generation     the lab iteration; absent on a template that predates the lab
    engine         v7 | v8                             the passivbot engine line

``title`` / ``title_zh`` are composed from universe, profile and capital. Two
templates listed together whose titles would read the same both take the first
four characters of their id, upper-cased, as a suffix: stable for the life of
the id, and a reader who sees ``BZWT`` can find ``tpl-bzwt…``.
"""

from __future__ import annotations

import secrets
from collections import Counter

from backtest_templates import detect_engine as engine_of
from rename_predefined import CATALOG

ID_PREFIX = "tpl-"
# Digits and lower-case letters, without the ones a reader confuses (0/o, 1/l/i).
ALPHABET = "23456789abcdefghjkmnpqrstuvwxyz"

FACETS = ("universe", "capital_usdt", "style", "profile", "generation", "engine")

UNIVERSES = {
    "mix3": ("3-coin basket", "三币组合"),
    "mix8": ("8-coin basket", "八币组合"),
    "mix10": ("10-coin basket", "十币组合"),
    "xrp": ("XRP only", "XRP 单币"),
}
PROFILES = {
    "guard": ("Guarded", "极保守"),
    "steady": ("Steady", "稳健"),
    "balanced": ("Balanced", "平衡"),
    "bold": ("Bold", "进取"),
    "extreme": ("Extreme", "极限"),
}
# passivbot v8's `live.strategy_kind`, absent meaning its default. v7.12 has a
# single strategy, the trailing grid that v8 carries on as `trailing_grid_v7`.
STYLES = {
    "trailing_grid_v7": "grid",
    "trailing_martingale": "martingale",
    "ema_anchor": "ema_anchor",
}
V8_DEFAULT_KIND = "trailing_martingale"

# The readable id each template had before ids went opaque -> its id. Frozen:
# bot configs and config-switch rows written before the move quote the readable
# one, and `resolve` walks them here.
IDS: dict[str, str] = {
    "bybit-mix10-1000u-balanced-v7-a": "tpl-sm2zeu7r",
    "bybit-mix10-1000u-balanced-v7-b": "tpl-2vewmtjy",
    "bybit-mix10-1000u-balanced-v7-c": "tpl-eej7t3s4",
    "bybit-mix10-1000u-balanced-v8": "tpl-bzwt9jn2",
    "bybit-mix10-1000u-bold-v7": "tpl-8wg3e88n",
    "bybit-mix10-1000u-bold-v8": "tpl-5syk2duu",
    "bybit-mix10-1000u-steady-v7": "tpl-xhdfc2ws",
    "bybit-mix10-500u-balanced-v7": "tpl-8bzdh8ay",
    "bybit-mix10-500u-bold-v7-a": "tpl-5jcrw7d5",
    "bybit-mix10-500u-bold-v7-b": "tpl-ec4fr9zm",
    "bybit-mix10-500u-guard-v7": "tpl-mvgw3zk4",
    "bybit-mix10-500u-steady-v7": "tpl-tavc364d",
    "bybit-mix10-700u-bold-v7-a": "tpl-35c6wt6w",
    "bybit-mix10-700u-bold-v7-b": "tpl-wuy2q2df",
    "bybit-mix10-700u-bold-v7-c": "tpl-3etaf9t3",
    "bybit-mix10-700u-bold-v7-d": "tpl-dhtapugv",
    "bybit-mix10-700u-bold-v8": "tpl-8brpubqf",
    "bybit-mix3-100u-steady-v7": "tpl-jtn3nwsg",
    "bybit-mix3-100u-steady-v8": "tpl-san8qrvj",
    "bybit-mix8-300u-bold-v7": "tpl-8ctkayqd",
    "bybit-mix8-300u-bold-v8": "tpl-m5xse3az",
    "bybit-xrp-10000u-bold-v7": "tpl-rwqvrc6u",
    "bybit-xrp-10000u-extreme-v7": "tpl-kypvfxgd",
    "bybit-xrp-10000u-steady-v7": "tpl-fhhjk83e",
    "bybit-xrp-100u-bold-v7": "tpl-sappt9w2",
    "bybit-xrp-100u-bold-v8": "tpl-hfpuyzcm",
    "bybit-xrp-100u-extreme-v7-a": "tpl-nkh4sfw4",
    "bybit-xrp-100u-extreme-v7-b": "tpl-jwzxkxkh",
    "bybit-xrp-100u-extreme-v8": "tpl-3x8we339",
}


def suffix(tid: str) -> str:
    return tid.removeprefix(ID_PREFIX)[:4].upper()


def new_id() -> str:
    return ID_PREFIX + "".join(secrets.choice(ALPHABET) for _ in range(8))


def resolve(name: str) -> str:
    """The id a stored template name refers to now. An optimizer-run name goes
    through the rename catalogue to its readable id, a readable id to its
    opaque one; one bot's copy lost the ``bybit-`` prefix before either."""
    for candidate in (name, f"bybit-{name}"):
        if candidate in CATALOG:
            name = CATALOG[candidate][0]
            break
    return IDS.get(name, name)


def style_of(config: dict) -> str:
    if engine_of(config) == "v7":
        return "grid"
    kind = str((config.get("live") or {}).get("strategy_kind") or V8_DEFAULT_KIND).strip().lower()
    return STYLES.get(kind, kind)


def capital_label(usdt: int) -> str:
    return f"${usdt // 1000}k" if usdt >= 1000 and usdt % 1000 == 0 else f"${usdt}"


def base_titles(meta: dict) -> tuple[str, str] | None:
    """(title, title_zh) from the naming properties, or None when one is missing."""
    try:
        universe = UNIVERSES[meta["universe"]]
        profile = PROFILES[meta["profile"]]
        capital = capital_label(int(meta["capital_usdt"]))
    except (KeyError, TypeError, ValueError):
        return None
    return (
        f"{universe[0]} · {profile[0]} · {capital}",
        f"{universe[1]} · {profile[1]} · {capital}",
    )


def titles(group: dict[str, dict]) -> dict[str, tuple[str, str]]:
    """id -> (title, title_zh) for templates listed together, given each one's
    `pbtb`. A title two of them would share carries each one's suffix; a
    template whose properties do not compose a title is left out."""
    bases = {tid: base_titles(meta) for tid, meta in group.items()}
    shared = Counter(base for base in bases.values() if base)
    out: dict[str, tuple[str, str]] = {}
    for tid, base in bases.items():
        if base is None:
            continue
        if shared[base] > 1:
            base = (f"{base[0]} · {suffix(tid)}", f"{base[1]} · {suffix(tid)}")
        out[tid] = base
    return out
