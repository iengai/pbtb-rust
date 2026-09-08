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
controls. Over a network that reasoning does not hold, so the HTTP edge resolves
a credential per request and builds the tool surface around the principal it
got — a tool is never constructed for an unauthenticated caller.

`StaticToken` is what the HTTP edge verifies against today: one shared bearer
standing for one tenant. Possession of the token is the whole claim, so it
identifies a deployment rather than a person, and rotating it revokes everyone at
once. Per-user identity is what OAuth is for, and is the next stage.

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

## Running it over HTTP

`mcp_http` is the same tool surface behind a Lambda Function URL. rmcp's
streamable-HTTP transport already is an `http::Request` handler, so the binary is
a thin bridge: `lambda_http` turns the Function URL event into a request, the
edge checks the bearer, and rmcp owns the protocol.

No session is ever minted. The `2026-07-28` revision has none, and an id handed
to an older client would stop resolving the moment the execution environment
recycled — so every request carries its own protocol version and capabilities in
`_meta`, and the server answers statelessly.

| Variable | Description |
|----------|-------------|
| `APP__MCP__USER_ID` | As above. |
| `APP__MCP__TOKEN_PARAM` | SSM parameter holding the bearer, read once at cold start. The name, not the value: the secret is never in the function's environment or in Terraform state. |

### Standing it up

The endpoint is **off by default**. `authorization_type = NONE` means AWS
forwards every request and the bearer check inside the function is the only thing
between a stranger and a live trading account, so creating it is a deliberate
act, not something an unrelated apply carries along.

```bash
# 1. terraform.tfvars
#      mcp_http_enabled = true
#      mcp_user_id      = "<the telegram user id>"
# 2. build the bootstrap the first apply uploads
cargo build --release --bin mcp_http
install -D target/release/mcp_http target/lambda/mcp_http/bootstrap
# 3. scoped apply — never a blanket one
terraform -chdir=terraform/envs/dev apply \
  -target=aws_ssm_parameter.mcp_bearer_token \
  -target=module.lambda_mcp_http \
  -target=aws_lambda_function_url.mcp_http \
  -target=aws_iam_role_policy.mcp_http_app
# 4. set the real token (the placeholder admits nobody)
aws ssm put-parameter --overwrite --type SecureString \
  --name /scalable-cluster/dev/mcp/bearer-token --value "$(openssl rand -base64 32)"
```

Then `terraform output mcp_http_url`. Code updates after that go through
`gh workflow run lambda-deploy.yml --ref main -f target=mcp-http`, not Terraform:
the function ignores `source_code_hash` drift.

To take it down, set `mcp_http_enabled = false` and apply the same targets.
