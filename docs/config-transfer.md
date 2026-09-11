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
  "name": "bybit-mix10-1000u-balanced-v8",
  "title": "10-coin basket · Balanced · $1k",
  "title_zh": "十币组合 · 平衡 · $1k",
  "exchange": "bybit",
  "description": "…",
  "strategies": [{ "name": "bybit-mix10-1000u-balanced-v8", "side": "long" }]
}
```

- `name` — the template's id, which is also its S3 key (see below).
- `title` / `title_zh` — what a reader is shown the template as, per language.
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

### The id, and the title

    id     bybit-<universe>-<capital>-<profile>-<engine line>[-<letter>]
    e.g.   bybit-mix10-1000u-balanced-v8, bybit-xrp-100u-bold-v7

Every field comes from the config: the coin universe (`xrp`, `mix3`, `mix8`,
`mix10`) and the tuned capital are the backtest's, the engine line is
`config_version`'s major, and the profile is the tier the measured worst
drawdown fell in when the template was published — `guard` (<10%), `steady`
(<25%), `balanced` (<35%), `bold` (<60%), `extreme` above it. One tuning
published on both engine lines keeps one profile, the more cautious of the two.
A letter disambiguates templates that agree on every field, tamest first.

The id addresses the template — it is the S3 key, the site URL, and what a
bot's stored config and its config-switch history quote — so it is **fixed for
the life of the template**, including its letter after a sibling retires. The
titles are wording and can be rewritten in place, and that is where a stale
sibling marker gets dropped. What must never go in either: the optimizer run
that produced it, and any claim about what it returns.
`scripts/rename_predefined.py` carries the mapping from the names this store
used before, and re-running it is a no-op.

Run it:

```bash
# preview
python scripts/transfer_config_to_s3.py --config E:/projects/passivbot/configs/xrp-cus.json
# upload a dual-sided strategy under its id and titles
python scripts/transfer_config_to_s3.py --config <raw.json> \
    --name bybit-xrp-100u-steady-v8 \
    --title "XRP only · Steady · \$100" --title-zh "XRP 单币 · 稳健 · \$100" \
    --upload --profile dev
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
