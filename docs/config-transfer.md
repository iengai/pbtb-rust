# Config transfer: passivbot config → our S3

How a raw passivbot config becomes a usable strategy/bot config in our platform,
and **exactly which custom properties we adjust** at each stage. Everything not
listed here is left as passivbot produced it.

## Stages and S3 layout

Bucket: `scalable-cluster-dev-bot-configs`

| Stage | S3 key | Who writes it | Custom properties touched |
|-------|--------|---------------|---------------------------|
| Predefined strategy | `predefined/<id>.json` | `scripts/transfer_config_to_s3.py` | `pbtb`, `lab` (and nothing else) |
| Per-bot config | `<user_id>/<bot_id>/<bot_id>.json` | telebot use cases | `live.user`, `live.forced_mode_<side>`, `bot.<side>.total_wallet_exposure_limit` (v8: `bot.<side>.risk.total_wallet_exposure_limit`), `live.leverage` |
| API keys | `<user_id>/<bot_id>/api-keys.json` | provided per bot | — |

At runtime the ECS task's `entrypoint.sh` downloads `<user_id>/<bot_id>/<bot_id>.json`
and `<user_id>/<bot_id>/api-keys.json`, then runs `python src/main.py configs/<bot_id>.json`,
which launches passivbot live (the user is read from `live.user`).

## Stage 1 — predefined transfer (the only schema addition)

A raw passivbot optimizer/strategy config is already valid; the transfer adds
**two top-level objects and nothing else**, split by who may read them: `pbtb`
is the console's, `lab` is ours.

### `pbtb` — what the console reads

```json
"pbtb": {
  "name": "tpl-bzwt9jn2",
  "title": "10-coin basket · Balanced · $1k · BZWT",
  "title_zh": "十币组合 · 平衡 · $1k · BZWT",
  "universe": "mix10",
  "capital_usdt": 1000,
  "style": "grid",
  "profile": "balanced",
  "generation": 7,
  "engine": "v8",
  "exchange": "bybit",
  "description": "…",
  "strategies": [{ "name": "tpl-bzwt9jn2", "side": "long" }]
}
```

- `name` — the template's id, which is also its S3 key (see below).
- `title` / `title_zh` — what a reader is shown the template as, per language,
  composed from the naming properties.
- `universe`, `capital_usdt`, `style`, `profile`, `generation`, `engine` — the
  naming properties (see below).
- `exchange` — whose market data the strategy was tuned on.
- `strategies` (array of `{name, side}`) — every side this strategy drives. A
  single-direction strategy lists one entry, a dual-sided one both.
- `description` (optional) — a free-text explanation, shown in the Telegram
  **State** view (`• Description:`) and returned by the API. Written only when
  the transfer is run with `--description`; absent configs show `—`.

Anything under `pbtb` can reach a user.

### `lab` — the strategy lab's record

```json
"lab": {
  "original_name": "bybit-cap1000-iter7-winner-v810",
  "source": "cap1000_iter7_winner",
  "tier": "cap1000", "iter": 7,
  "run": "1b750084", "member": "8914adeb2d",
  "seeds": ["cap1000_iter6_winner", "…"],
  "genome": "6501db3f96", "branch": "let-profits-run",
  "notes": "…"
}
```

Where the tuning came from: the lab config it was harvested as (`source`, with
its `tier` and `iter`), the optimizer `run` and population `member`, the
`seeds` that run warm-started from, the `genome` (the member its family
descends from; a template with no family is its own) and `branch` within it,
`migrated_from` on a v8 conversion of a v7 template, the lab's `status` verdict
and free-text `notes`. The name before the rename stays as `original_name`.

No surface reads `lab`, and `BotConfig::from_template` leaves it out of a bot's
copy, so the MCP `get_bot_config` tool never returns it. Write it with
`transfer_config_to_s3.py --lab <file.json>`; `scripts/annotate_templates.py`
holds the catalogue's lineage and re-applies it.

Everything outside `pbtb` and `lab` — `live`, `bot`, `approved_coins`,
`coin_overrides`, `optimize`, `backtest`, `analysis`, `logging`,
`disable_plotting` — is byte-for-byte what passivbot produced.

Older templates carried the same fields as top-level `strategy_name` /
`strategies` / `name` / `description`. `BotConfig::meta` still reads those, for
the per-bot configs written before the block; no template in S3 has them.

### The id, and the names

    id     tpl-<8 random characters>        e.g. tpl-bzwt9jn2

The id addresses the template — it is the S3 key, the site URL, the Telegram
callback, and what a bot's stored config and its config-switch rows quote — and
says nothing about it, so nothing learnt about the template can make it wrong.
It is **fixed for the life of the template**.

What a reader is told lives in the naming properties, each read off the config
or the strategy lab:

| Property | Values | From |
|---|---|---|
| `universe` | `mix3`, `mix8`, `mix10`, `xrp` | the backtest's coins |
| `capital_usdt` | `100` … `10000` | the backtest's starting balance |
| `style` | `grid`, `martingale`, `ema_anchor` | v8 `live.strategy_kind`; every v7 config is the grid |
| `profile` | `guard` (<10%), `steady` (<25%), `balanced` (<35%), `bold` (<60%), `extreme` | the tier the worst measured drawdown falls in; one tuning on both engine lines keeps the more cautious |
| `generation` | the lab iteration | `lab.iter`, counted per capital tier; absent on a template that predates the lab |
| `engine` | `v7`, `v8` | `config_version`'s major, else the config's shape |

`title` / `title_zh` are composed from universe, profile and capital:
`10-coin basket · Balanced · $1k`. Two templates listed together whose titles
would read the same both take the first four characters of their id as a
suffix (`· BZWT`), so a reader can tell them apart and find the id. Neither a
title nor a property carries a claim about what the template returns.

`scripts/template_naming.py` holds the vocabulary and the ids the store used
before (`bybit-mix10-1000u-balanced-v8`, and the optimizer-run names before
those), and resolves an old name a stored config or history row still quotes.
`scripts/annotate_templates.py --apply` re-derives the properties and
recomposes every title; run it after adding or retiring a template.

Run it:

```bash
# preview
python scripts/transfer_config_to_s3.py --config E:/projects/passivbot/configs/xrp-cus.json
# upload a dual-sided strategy under a new id, then recompose the titles
python scripts/transfer_config_to_s3.py --config <raw.json> \
    --universe xrp --capital 100 --risk-profile steady \
    --upload --profile dev
python scripts/annotate_templates.py --apply --profile dev
# single-direction
python scripts/transfer_config_to_s3.py --config <raw.json> --sides long --upload --profile dev
```

> A combined bot mixes strategies per side (e.g. one strategy's `long`, another's
> `short`). Each predefined file still describes only its own strategy; the
> combination lives in the per-bot config's `strategies` array.

### Retiring one

A template another one beats on both gain and worst drawdown at the same
capital tier is not worth offering. `scripts/retire_templates.py <id> …
--apply` moves the object to `retired/` — out of every listing, content and
history intact — deletes its backtest artifact and rebuilds the site index;
`--restore` puts it back. It refuses to retire a template a bot's stored config
names, resolving the old names those configs still carry through the rename
catalogue first. Only drawdowns measured over the **same backtest window** may
be compared: a run that stops at 2025-04-30 never met the 2025-10-10 crash.
Then re-run `annotate_templates.py --apply`: a title suffix the retired
template forced on a sibling is dropped.

## Stage 2 — per-bot adjustments (telebot, not the transfer script)

These are applied to the per-bot config by the bot, never at transfer time:

| Property | Set by | Meaning |
|----------|--------|---------|
| `live.user` | `BotConfig::from_template` / `set_live_user` (apply template) | identity the running task reports under = `bot_id` |
| `live.forced_mode_<side>` | `SetStrategySideUseCase` (Telegram **Sides**) | `""`/`"normal"` = side on; `"graceful_stop"` = side off (close out, no new entries) |
| `bot.<side>.total_wallet_exposure_limit` (v7) / `bot.<side>.risk.total_wallet_exposure_limit` (v8) | `apply_risk_level` (Telegram **Risk level**) | risk per side; the path follows the config's schema (see below) |
| `live.leverage` | `apply_risk_level` | derived: `max(long, short) + 1.0` |

Code: `src/domain/botconfig.rs`, `src/usecase/apply_template.rs`,
`src/usecase/set_strategy_side.rs`.

## passivbot v8 schema

passivbot v8 configs carry `"config_version": "v8.1.0"` and nest the per-side
wallet exposure under a `risk` object: `bot.<side>.risk.total_wallet_exposure_limit`
(v7 keeps it flat at `bot.<side>.total_wallet_exposure_limit`). Both shapes
coexist in S3 while bots are migrated (`passivbot tool migrate-config-v7`
produces the v8 shape).

- The transfer script is a pass-through: it adds the marker properties above
  and never touches `bot.*`, so a v8 config uploads as v8 and a v7 one as v7.
- telebot handles both shapes per config: `BotConfig::risk_level` /
  `set_risk_level` use the `risk.*` path when `bot.<side>.risk` is an object and
  the flat path otherwise. On a v8 config a write also removes any stale flat
  key, because the v8 engine still honours a flat key left beside `risk.*`.
  A v7 config never gains a `risk` object.

## Runtime image

The predefined/per-bot config schema is consumed by the passivbot live image
(`passivbot-live:v8.1.0-arm64`, built from `deploy/passivbot-image/Dockerfile.ecs`
in this repo, overlaid onto the upstream checkout by
`scripts/build_passivbot_image.py`). passivbot 8.1.0 expects the v8 schema.
