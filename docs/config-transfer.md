# Config transfer: passivbot config → our S3

How a raw passivbot config becomes a usable strategy/bot config in our platform,
and **exactly which custom properties we adjust** at each stage. Everything not
listed here is left as passivbot produced it.

## Stages and S3 layout

Bucket: `scalable-cluster-dev-bot-configs`

| Stage | S3 key | Who writes it | Custom properties touched |
|-------|--------|---------------|---------------------------|
| Predefined strategy | `predefined/<name>.json` | `scripts/transfer_config_to_s3.py` | `strategy_name`, `strategies`, `description` |
| Per-bot config | `<user_id>/<bot_id>/<bot_id>.json` | telebot use cases | `live.user`, `live.forced_mode_<side>`, `bot.<side>.total_wallet_exposure_limit` (v8: `bot.<side>.risk.total_wallet_exposure_limit`), `live.leverage` |
| API keys | `<user_id>/<bot_id>/api-keys.json` | provided per bot | — |

At runtime the ECS task's `entrypoint.sh` downloads `<user_id>/<bot_id>/<bot_id>.json`
and `<user_id>/<bot_id>/api-keys.json`, then runs `python src/main.py configs/<bot_id>.json`,
which launches passivbot live (the user is read from `live.user`).

## Stage 1 — predefined transfer (the only schema additions)

A raw passivbot optimizer/strategy config is already valid; the transfer adds
**two top-level marker properties and nothing else**:

- `strategy_name` (string) — the strategy stem. Shown in the Telegram **State**
  view and used to attribute a bot's strategy.
- `strategies` (array of `{name, side}`) — every side this strategy drives.
  A single-direction strategy lists one entry; a dual-sided one lists both:

  ```json
  "strategy_name": "xrp-241201251009-r46x-lq",
  "strategies": [
    { "name": "xrp-241201251009-r46x-lq", "side": "long" },
    { "name": "xrp-241201251009-r46x-lq", "side": "short" }
  ]
  ```

- `description` (string, optional) — a free-text strategy explanation, shown in
  the Telegram **State** view (`• Description:`). Written only when the transfer
  is run with `--description`; absent configs show `—`.

Verified by diffing `predefined/xrp-241201251009-r46x-lq.json` against the raw
`configs/xrp-241201251009-r46x-lq.json`: the **only** difference is these two
keys. `live`, `bot`, `approved_coins`, `coin_overrides`, `optimize`, `backtest`,
`analysis`, `logging`, `disable_plotting` are byte-for-byte identical.

Run it:

```bash
# preview
python scripts/transfer_config_to_s3.py --config E:/projects/passivbot/configs/xrp-cus.json
# upload a dual-sided strategy
python scripts/transfer_config_to_s3.py --config <raw.json> --upload --profile dev
# single-direction
python scripts/transfer_config_to_s3.py --config <raw.json> --sides long --upload --profile dev
```

> A combined bot mixes strategies per side (e.g. one strategy's `long`, another's
> `short`). Each predefined file still describes only its own strategy; the
> combination lives in the per-bot config's `strategies` array.

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
