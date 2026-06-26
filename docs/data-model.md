# Data Model

Persistent state lives in two AWS stores: a single DynamoDB table for bot metadata and observed runtime, and an S3 bucket for configurations, templates, and API keys. In both stores, `user_id` is the tenant isolation boundary — every record is scoped under the owning user.

## DynamoDB (single table)

One table holds two row kinds under a shared partition key `pk = "user_id#<user_id>"`. The sort key (`sk`) distinguishes the two kinds.

```
Bot row      pk = "user_id#<user_id>", sk = "<bot_id>"
             Attributes: name, exchange, api_key, secret_key, enabled,
                         created_at, updated_at
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

`user_id` is the tenant isolation boundary. Every DynamoDB row lives under `pk = "user_id#<user_id>"`, and every S3 object lives under the `{user_id}/` prefix, so a caller must only ever touch their own data. Derive the `user_id` from the authenticated Telegram user, never from client-supplied input, and validate it before any read or write. Treat any cross-`user_id` access as a privilege-escalation bug.
