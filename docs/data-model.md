# Data Model

Persistent state lives in two AWS stores: a single DynamoDB table for bot metadata and observed runtime, and an S3 bucket for configurations, templates, and API keys. In both stores, `user_id` is the tenant isolation boundary — every record is scoped under the owning user.

## DynamoDB (single table)

One table holds the tenant's rows under a shared partition key `pk = "user_id#<user_id>"` (the sort key `sk` distinguishes the kinds), plus the account, identity and link-ticket rows under partition prefixes of their own.

```
Bot row      pk = "user_id#<user_id>", sk = "<bot_id>"
             Attributes: name, exchange, api_key, secret_key, enabled,
                         runtime, public_url, created_at, updated_at
             (enabled = desired state; there is no status attribute)

Runtime row  pk = "user_id#<user_id>", sk = "ecs_task_metadata#<bot_id>"
             Attributes: status (starting/running/stopping/stopped), task_id,
                         task_updated_at, task_current_version
             (observed ECS task state)
```

### Bot row

The bot's configured identity and desired state.

| Attribute | Description |
|-----------|-------------|
| `name` | Bot display name |
| `exchange` | Target exchange (currently Bybit) |
| `api_key` | Exchange API key |
| `secret_key` | Exchange API secret |
| `enabled` | Desired state (user intent) — whether the user turned the bot on |
| `runtime` | Which image runs the bot's engine line: `py` (passivbot, Python) or `rs` (pb-runner, Rust). Optional; a row without it reads as `py`. Read at launch only (`/runtime <bot_id> py\|rs` sets it; applies on the next Run) |
| `public_url` | The bot's Bybit copy-trading page, an https URL on `bybit.com`. Optional; written by `/public <bot_id> <url>` (the operator's account only) or by the ops `set-public-url` (any account; the collector publishes only an operator's bots, so on a member's bot the link is inert); `off` removes it. Read as stored |
| `created_at` | Creation timestamp |
| `updated_at` | Last-modified timestamp |

`enabled` records desired state only. There is **no `status` attribute** on the bot row; observed run/stop reality lives on the separate runtime row.

### Runtime row

The observed `BotRuntime` for a bot — whether the ECS task is actually running. Written by the ECS Task State Change Lambda (`task_state_change_handler`): the RUNNING path records the observed-running task, and the STOPPED path reconciles the stop.

| Attribute | Description |
|-----------|-------------|
| `status` | Observed phase (`starting` / `running` / `stopping` / `stopped`) |
| `task_id` | ECS task identifier |
| `task_updated_at` | Timestamp of the last observed update |
| `task_current_version` | Version counter for the runtime row |

### Account row

The account behind a tenant: whether it exists, what level it holds, and
whether it is allowed in. Under its own partition prefix (`user#`, not the
tenant's `user_id#`), so the bot readers never meet it.

```
Account row  pk = "user#<user_id>", sk = "profile"
             Attributes: vip_level (0..9), status (active | suspended),
                         role (operator | member; absent = member), email,
                         created_at, updated_at
```

`role` marks the operator's account: the person running this deployment,
the one account whose bots may be put on the public showcase page. Every
other row is absent or, after a demotion, `member`; an unknown value reads as
`member` (logged, not a corrupt row). Set with `set-role` and by nothing else; the API's own
authorization is the WorkOS org role of the same name (docs/workos.md),
granted separately.

`user_id` is opaque and minted here (32 hex chars for new accounts); it is
neither a Telegram id nor an identity provider's subject, so a change of
provider or of WorkOS environment touches identity rows only, never the
tenant's data. Accounts that predate the row keep the id their data already
lives under.

### Identity rows

An external identity mapped onto an account. Two providers: `workos` (the
subject a token presents; the primary identity, written at signup and never
released) and `telegram` (the sender id the bot sees; can be released and
bound again).

```
Identity     pk = "identity#<provider>#<subject>", sk = "profile"
             Attributes: user_id, email, linked_at
Listing      pk = "user_id#<user_id>", sk = "identity#<provider>#<subject>"
```

An identity names exactly one account (conditional write); one account may
hold several. The listing row sits in the tenant's partition and is skipped
by the bot readers (`is_bot_row`).

Signup writes the account row first and the `workos` identity second, so two
signups racing for one subject leave the loser's account row behind with no
identity pointing at it. Such a row is inert — nothing resolves to it and it
holds no data — and is tolerated rather than cleaned up.

Operator commands for these rows: `python scripts/ops/pbtb_ops.py user-show |
user-create | set-vip | set-role | user-status`; for a bot row's showcase link, `set-public-url`.

### Row shapes and the readers

Two readers walk rows without a sort-key condition: `find_by_user_id` queries
a whole tenant partition, and `find_all` scans the table for the daily
return-curve collector. Both tell bot rows from the rest by shape: a bot row's
`sk` is the bare `bot_id`, which `Bot::validate_name` keeps free of `#`, and
every other row in the partition carries a `<kind>#` prefix
(`ecs_task_metadata#`, `config_switch#`, `identity#`). `is_bot_row` in
`src/infra/botrepository.rs` admits only the former, so a shape added later is
skipped without being named; `find_all` additionally ignores any `pk` that is
not a `user_id#` partition, and `scripts/ops/pbtb_ops.py bot-status` applies
the same two rules.

A new `<kind>#` row therefore needs no change to the readers, as long as it
keeps the prefix; `tests/identity_link_test.rs` and `tests/botrepository_test.rs`
hold the tests that the readers survive foreign rows. What still fails them is
a bot row that does not parse: `parse_bot_row` raises `CorruptRecord` and takes
the whole result with it — the tenant's bot list, every launch, and the restart
lambda for that tenant via `find_by_user_id`; every tenant's daily snapshot via
`find_all`. The rows have no TTL, so that lasts until the row or the reader is
fixed. Binaries built before `is_bot_row` still carry one predicate per known
shape and fail on a shape they were not taught; deploy them before writing a
new one.

## S3 (configurations, templates, API keys)

A single bucket (`{project}-{env}-bot-configs`) holds reusable templates under `predefined/`, the archived ones under `retired/`, and per-bot data under `{user_id}/{bot_id}/`.

```
Bucket: {project}-{env}-bot-configs
├── predefined/              # Configuration templates, keyed by id
│   ├── tpl-bzwt9jn2.json
│   └── tpl-sappt9w2.json
├── retired/                 # Archived: same objects, out of every listing
│   └── tpl-wuy2q2df.json
└── {user_id}/              # User-specific data
    └── {bot_id}/
        ├── {bot_id}.json   # Bot configuration
        └── api-keys.json   # API credentials
```

- `predefined/` — reusable configuration templates. The key is the template's
  id, an opaque `tpl-<8 characters>` fixed for its life (see
  [config-transfer.md](config-transfer.md)). The template's own metadata sits
  under its top-level `pbtb` object: `title` / `title_zh`, what a reader is
  shown it as; the naming properties they are composed from (`universe`,
  `capital_usdt`, `style`, `profile`, `generation`, `engine`); `exchange`,
  `description`, `strategies`; `min_vip_level`, the lowest account level
  that may apply it (absent = open to all); and `audience`, `"operator"` on a
  retired template, offered to the operator's account only (absent =
  published, everyone; a member is not listed it and may not apply it). An
  operator edits the object
  to change either gate, no deploy needed. Beside it, `lab` is the strategy
  lab's record of the tuning (run, seeds, genome, verdicts); no surface reads
  it and a bot's copy of the template drops it.
- `retired/` — an archived template, offered to no one. `S3TemplateRepository::list`
  scans `predefined/` only, so an object here is absent from the Telegram
  chooser, the API and the site while its content and version history stay
  intact. `scripts/archive_templates.py` moves an id either way and refuses to
  archive one a bot's stored config still names.
- `{user_id}/{bot_id}/{bot_id}.json` — the bot's configuration.
- `{user_id}/{bot_id}/api-keys.json` — the bot's exchange API credentials.

## S3 (return curves)

The chart bucket (`{project}-{env}-return-charts`) is the daily collector's, keyed by tenant like the config bucket: `charts/{user_id}/{bot_id}.json` is the series the API serves to the bot's owner (`GET /api/v1/bots/{id}/returns`), `_state/{user_id}/{bot_id}.json` the accumulated ledger only the collector reads. The prefix is one Terraform local shared by the collector and the API function. The console reads a tenant's curves through the API after sign-in.

`public/` is the one prefix that leaves the bucket: `public/index.json` (every showcase bot: opaque id, name, exchange, link, current return, a 30-day sparkline) and `public/bots/{id}.json` (the curve as index and return per day, the config switches and capital resets each with the rounded capital the bot ran at, never a balance or a realized figure), written on every run for each bot whose account row is the operator's and whose bot row carries `public_url`. `{id}` is `sha256("{user_id}#{bot_id}")` cut to twelve hex characters: a collision-safe key across tenants (bot ids are per-tenant names), not a shield for the `user_id`, which a keyed hash would be. A bot that stops being public loses its file at the next run. The pages-publish workflow copies the prefix to the site (`site/data/`).

## Tenant isolation

`user_id` is the tenant isolation boundary. Every DynamoDB row lives under `pk = "user_id#<user_id>"`, and every S3 object lives under the `{user_id}/` prefix, so a caller must only ever touch their own data. Derive the `user_id` from the authenticated principal (the account a Telegram sender's `telegram` identity row resolves to, or a verified token's linked subject), never from client-supplied input, and validate it before any read or write. Treat any cross-`user_id` access as a privilege-escalation bug.
