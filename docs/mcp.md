# MCP server

The bot-management use cases exposed as MCP tools, so a coding agent can list,
inspect, start, stop and reconfigure bots. `src/interface/mcp/` is a sibling of
`src/interface/telegram/`: both adapters drive the same use cases, so a rule
enforced in a use case holds for a tool call exactly as it does for a button
press — the exclusive start lock above all.

Built against the official Rust SDK (`rmcp`), which targets MCP revision
`2026-07-28` and owns the JSON-RPC envelope, the schema types and protocol
version negotiation.

## What is deliberately absent

Three things do not exist here, and the tests in `tests/mcp_tools.rs` assert
their absence against the tool registry rather than trusting review:

- **No tool takes a `user_id`.** The tenant comes from the authenticated
  principal, so a caller cannot name someone else's bots. Tenant isolation stays
  a thing the surface cannot express, not a check that has to catch it.
- **No tool takes exchange credentials.** A tool argument lands in the model's
  context and in the transcript. That is why there is no `add_bot`: adding a bot
  means entering keys, which stays on the Telegram path.
- **No whole-config overwrite.** `update_bot_config` replaces a live bot's
  position parameters in one call. Only named, validated fields are exposed
  (`set_risk_level`, `set_strategy_side`, `apply_template`, `set_bot_runtime`).

Every write is logged with principal, tool, bot id and outcome.

## Tools

| Tool | Scope | Notes |
| --- | --- | --- |
| `list_bots` | `bots:read` | id, name, exchange, desired state, runtime, observed phase |
| `get_bot_status` | `bots:read` | observed phase, task id, restart generation |
| `get_bot_config` | `bots:read` | the stored passivbot config |
| `list_templates` | `bots:read` | |
| `start_bot` | `bots:write` | claims the DynamoDB start lock; idempotent |
| `stop_bot` | `bots:write` | idempotent |
| `apply_template` | `bots:write` | applies on the bot's next start |
| `set_risk_level` | `bots:write` | per-side wallet exposure limits |
| `set_strategy_side` | `bots:write` | enable/disable one side |
| `set_bot_runtime` | `bots:write` | `py` (passivbot) or `rs` (pb-runner) |
| `delete_bot` | `bots:write` | destructive; `confirm` must equal `bot_id` |

Config changes take effect on a bot's next start. A running task keeps the
config and the binary it started with.

## Authentication

`trait Authenticator` resolves a transport's credentials into a `Principal`
(`user_id` + scopes). It is the seam between transports: the tools depend on the
trait, so adding a transport changes no tool.

`LocalOperator` is the stdio implementation. It trusts whoever started the
process, which is sound only because on stdio the transport *is* the credential:
the client spawns the binary as a child of a shell the operator already
controls. Over a network that reasoning does not hold, which is why an HTTP
transport takes its own implementation rather than reusing this one.

## Running it over stdio

`mcp_stdio` reads the same `APP__*` environment as telebot, plus:

| Variable | Description |
|----------|-------------|
| `APP__MCP__USER_ID` | The Telegram user id this server acts as. Required; must be on `APP__TELEGRAM__ALLOWED_USER_IDS`, so removing someone from the bot's allowlist takes their MCP access with it. |

Nothing is written to stdout — that is the protocol stream. Logs go to stderr,
filtered by `RUST_LOG`.

```bash
cargo build --release --bin mcp_stdio
```

Then register the built binary with a client, for example:

```bash
claude mcp add pbtb -- /path/to/target/release/mcp_stdio
```

The client needs the `APP__*` variables in its environment, and AWS credentials
for the real table and bucket — this talks to the same DynamoDB table and S3
bucket telebot does, so a `start_bot` here launches a real live-trading task.
