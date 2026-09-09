# Data Model

Persistent state lives in two AWS stores: a single DynamoDB table for bot metadata and observed runtime, and an S3 bucket for configurations, templates, and API keys. In both stores, `user_id` is the tenant isolation boundary — every record is scoped under the owning user.

## DynamoDB (single table)

One table holds the tenant's rows under a shared partition key `pk = "user_id#<user_id>"` (the sort key `sk` distinguishes the kinds), plus the account, identity and link-ticket rows under partition prefixes of their own.

```
Bot row      pk = "user_id#<user_id>", sk = "<bot_id>"
             Attributes: name, exchange, api_key, secret_key, enabled,
                         runtime, created_at, updated_at
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
                         email, created_at, updated_at
```

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
by `find_by_user_id` (`is_identity_row`).

Operator commands for these rows: `python scripts/ops/pbtb_ops.py user-show |
user-create | set-vip | user-status`.

### Row shapes and the readers

Two readers walk rows without a sort-key condition: `find_by_user_id` queries
a whole tenant partition, and `find_all` scans the table for the daily
return-curve collector. Both tell bot rows from the rest by shape: a bot row's
`sk` is the bare `bot_id`, which must not contain `#`, and every other row in
the partition carries a `<kind>#` prefix (`ecs_task_metadata#`,
`config_switch#`, `identity#`) that the predicates in
`src/infra/botrepository.rs` (`is_runtime_row`, `is_config_switch_row`,
`is_identity_row`) skip. `find_all` additionally ignores any `pk` that is not a
`user_id#` partition.

A shape that lands under `user_id#` without being registered there reaches
`parse_bot_row`, fails as `CorruptRecord`, and takes the whole result with it:
the tenant's bot list, every launch, and the restart lambda for that tenant via
`find_by_user_id`; every tenant's daily snapshot via `find_all`. The rows have
no TTL, so the failure lasts until the reader is fixed. When adding a shape,
first write the test that the old readers still work with the new row present
(`tests/identity_link_test.rs` has two), watch it fail, register the shape,
then write the row.

## S3 (configurations, templates, API keys)

A single bucket (`{project}-{env}-bot-configs`) holds reusable templates under `predefined/` and per-bot data under `{user_id}/{bot_id}/`.

```
Bucket: {project}-{env}-bot-configs
├── predefined/              # Configuration templates
│   ├── template1.json
│   └── template2.json
└── {user_id}/              # User-specific data
    └── {bot_id}/
        ├── {bot_id}.json   # Bot configuration
        └── api-keys.json   # API credentials
```

- `predefined/` — reusable configuration templates.
- `{user_id}/{bot_id}/{bot_id}.json` — the bot's configuration.
- `{user_id}/{bot_id}/api-keys.json` — the bot's exchange API credentials.

## Tenant isolation

`user_id` is the tenant isolation boundary. Every DynamoDB row lives under `pk = "user_id#<user_id>"`, and every S3 object lives under the `{user_id}/` prefix, so a caller must only ever touch their own data. Derive the `user_id` from the authenticated principal (the account a Telegram sender's `telegram` identity row resolves to, or a verified token's linked subject), never from client-supplied input, and validate it before any read or write. Treat any cross-`user_id` access as a privilege-escalation bug.
