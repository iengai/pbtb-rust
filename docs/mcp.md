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

The browser console's REST surface (`/api/v1`, [web-api.md](web-api.md)) shares
this function, this bearer check and these use cases; it is where key entry
lives, and it never returns a config.

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
process — and grants the top level, deliberately: the operator's own shell and
the deployment's shared bearer are not tiers of the service, so `Principal::full`
carries `MAX_VIP_LEVEL`. It is sound only because on stdio the transport *is* the credential:
the client spawns the binary as a child of a shell the operator already
controls. Over a network that reasoning does not hold, so the HTTP edge resolves
a credential per request and builds the tool surface around the principal it
got — a tool is never constructed for an unauthenticated caller.

`trait TokenVerifier` is the HTTP side of that seam: a presented bearer in, a
`Principal` out. Two implementations, chosen by whether an issuer is configured.

`StaticToken` is one shared bearer standing for one tenant. Possession of the
token is the whole claim, so it identifies a deployment rather than a person, and
rotating it revokes everyone at once.

`OAuthTokens` makes each caller a person. The issuer's side — which
applications exist, how scopes and the audience are registered, and the traps
in it — is in [workos.md](workos.md). Three checks stand between a token and a
tenant, and all three have to pass:

1. **The token verifies** against the issuer's published keys — signature,
   issuer, audience and expiry. The algorithm is taken from the published key,
   never from the token's own header, so the token cannot choose how it is
   checked. The audience must be this server's own URL: a token the same user
   holds for some other API is a valid token, just not one for here. All four
   claims are required to be *present* — naming an expected value only rejects a
   claim that is there and wrong, so without that a token minted with no `aud`
   would sail through whoever it was minted for.
2. **The subject has an account.** `pk = identity#workos#<subject>` maps a
   provider subject onto a `user_id`. Authenticating with the provider is not an
   application for an account — an unlinked subject is refused, and nothing is
   provisioned for it. The web console's `POST /api/v1/signup` is the one
   deliberate way to create that row ([web-api.md](web-api.md)).
3. **The account is active.** The account row (`user#<user_id>`) has to exist
   and not be suspended: suspending someone at the bot suspends them here too,
   rather than leaving a second door they still hold a key to.

Two things about the key set are worth naming, because they are what makes step 1
mean anything. The discovery document has to agree that it belongs to the
configured issuer and has to keep its `jwks_uri` on the issuer's own origin —
whoever serves that document otherwise chooses which signatures are genuine. And
a symmetric algorithm is refused outright: a key set is public, so an `oct` entry
would publish the very secret that signs tokens, and anyone who could read the
document could mint one for any subject.

Scopes come from the token's `scope` claim, and only the two this server defines
survive it. A token that asked for nothing gets nothing and is refused by the
first tool it reaches. A floor of read access would make the claim decorative:
this surface lists every bot in the tenant and hands over its full trading
config.

A refusal distinguishes the two cases, because they mean different things to a
client: **401** says get a better token, **403** says the token is fine and the
answer is still no. Both carry `WWW-Authenticate` with `resource_metadata`, so a
client can find the authorization server from the failure alone. The 403 carries
no `error=` code — RFC 6750's codes are about token problems, and this token has
none.

## Running it over stdio

`mcp_stdio` reads the same `APP__*` environment as telebot, plus:

| Variable | Description |
|----------|-------------|
| `APP__MCP__USER_ID` | The Telegram user id this server acts as. Required; must be on `APP__TELEGRAM__ALLOWED_USER_IDS`, so removing someone from the bot's allowlist takes their MCP access with it. |

telebot reads one variable of its own, `APP__LINK__URL`: where the **Link
account** button points. Empty hides the flow behind a message saying so.

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
| `APP__MCP__RESOURCE_URL_PARAM` | SSM parameter holding this server's own public URL. Indirected through SSM because a function cannot name the URL of the function it belongs to — Terraform would have to build the environment from a resource that depends on it. |
| `APP__MCP__TOKEN_PARAM` | SSM parameter holding the shared bearer, read once at cold start. The name, not the value: the secret is never in the function's environment or in Terraform state. Unused once an issuer is set. |
| `APP__MCP__ISSUER` | OAuth issuer to accept tokens from. Empty selects the shared bearer. |
| `APP__LINK__CLIENT_ID` | OAuth client id for the account-linking flow. Empty leaves its routes unserved. |
| `APP__LINK__CLIENT_SECRET_PARAM` | SSM parameter holding the client secret. |
| `APP__TELEGRAM__BOT_USERNAME` | The bot's `@username` (without the `@`), for the `https://t.me/<username>?start=<token>` deep link a Telegram bind ticket is handed out as. Empty hands out the bare token. |

### Discovery

`GET /.well-known/oauth-protected-resource` answers **without a token** — a
caller with no token is exactly who needs it — and returns the RFC 9728 document:
the resource identifier, the authorization servers, and the scopes this server
defines.

### Standing it up

The endpoint is **off by default**. `authorization_type = NONE` means AWS
forwards every request and the bearer check inside the function is the only thing
between a stranger and a live trading account, so creating it is a deliberate
act, not something an unrelated apply carries along.

Set `mcp_issuer` in the same step rather than standing the shared bearer up
first. The bearer parameter is created holding `REPLACE_ME`, and `StaticToken`
refuses only the *empty* token — so between that apply and the `put-parameter`
below, the door is open to anyone who has read this repository. With an issuer
the parameter is never created and the endpoint comes up admitting nobody.

```bash
# 1. terraform.tfvars
#      mcp_http_enabled = true
#      mcp_issuer       = "https://<project>.authkit.app"
#    the gitignored allowlist tfvars
#      mcp_user_id      = "<the telegram user id>"
# 2. build the bootstrap the first apply uploads — the same lambda-export stage
#    CI uses, because the function runs on AL2023 and the host does not
docker build --target lambda-export --platform linux/amd64 \
  --build-arg BIN_NAME=mcp_http -o type=local,dest=artifact -f .devcontainer/Dockerfile .
install -D artifact/bootstrap target/lambda/mcp_http/bootstrap
# 3. scoped apply — never a blanket one
terraform -chdir=terraform/envs/dev apply \
  -target=module.lambda_mcp_http \
  -target=aws_lambda_function_url.mcp_http \
  -target=aws_lambda_permission.mcp_http_invoke \
  -target=aws_ssm_parameter.mcp_resource_url \
  -target=aws_iam_role_policy.mcp_http_app
```

Add `-target=aws_ssm_parameter.mcp_bearer_token` and the `put-parameter` above
only for a deployment that really wants one shared token instead of an issuer.

The apply also enables the bots table's TTL, which the link tickets need. It is
in-place and it sweeps anything whose `expires_at` has passed, so check that no
existing row carries that attribute before the first one:

```bash
aws dynamodb scan --table-name scalable-cluster-dev-bots \
  --filter-expression "attribute_exists(expires_at)" --select COUNT
```

Then `terraform output mcp_http_url`. Code updates after that go through
`gh workflow run lambda-deploy.yml --ref main -f target=mcp-http`, not Terraform:
the function ignores `source_code_hash` drift.

Verify it with the two requests that need no credentials — the metadata document
answers, and a tool call is refused:

```bash
curl "$URL/.well-known/oauth-protected-resource"     # 200 + the RFC 9728 document
curl -i -X POST "$URL/" -H 'content-type: application/json' -H 'Mcp-Method: tools/list' \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{"_meta":{}}}'   # 401
```

A 403 from AWS on *both*, with `x-amzn-ErrorType: AccessDeniedException` and no
log group on the function, is the edge refusing before the function runs — the
resource policy, not the code. Since October 2025 a function URL needs
`lambda:InvokeFunction` as well as `lambda:InvokeFunctionUrl`, and creating the
URL grants only the latter; `aws_lambda_permission.mcp_http_invoke` is the other
half. Note also that AWS renames the challenge header to
`x-amzn-Remapped-www-authenticate` on the way out.

To take it down, set `mcp_http_enabled = false` and apply the same targets.

### Turning on per-user OAuth

Set `mcp_issuer` in `terraform.tfvars` and apply the same targets. Three things
change:

- The shared bearer parameter is **destroyed**. With an issuer there is no shared
  door, so it is not left standing unlocked.
- The endpoint starts requiring a token audienced for `mcp_http_url`. Register
  that URL as a resource with the authorization server, and register the two
  scopes, or every token arrives read-only.
- Nobody can reach it until their identity is linked. Signing up on the web
  (`POST /api/v1/signup`) is what writes that link and the account row. Both
  can also be written by hand — the identity row below, and the account with
  `python scripts/ops/pbtb_ops.py user-create`:

```bash
aws dynamodb put-item --table-name scalable-cluster-dev-bots --item '{
  "pk":        {"S": "identity#workos#<workos user id>"},
  "sk":        {"S": "profile"},
  "user_id":   {"S": "<the telegram user id>"},
  "linked_at": {"N": "0"}
}'
```

Note the row is read with a strongly consistent get: deleting it revokes access
on the next request, not eventually.

## Identities

Two providers map onto an account (`docs/data-model.md`): `workos`, the subject
a token presents, written once at signup and never released; and `telegram`,
the sender id the bot sees, bound from the web (`/start <token>` in the bot
redeems a one-time ticket the console minted for the signed-in account) and
released by `/unlink` in the bot or from the account page. One account holds at
most one Telegram id, and a Telegram id names one account.

### The bot-initiated link flow (retired)

`src/interface/link/` is the only place this crate is an OAuth *client* rather
than a resource server. It was the bot-initiated direction — a **Link account**
button minting a ticket the browser redeemed — and the bot no longer offers it:
accounts are created on the web, so the `workos` identity exists before any
Telegram id does. The module stays until signup replaces it. What follows
describes it as it still runs.

🔴 **The browser never gets to say which account it is linking.** The tenant is
established before the browser is involved and travels server-side the whole way:

1. **Link account** in the bot mints a one-time token for the user Telegram
   already authenticated, stores only its SHA-256 next to that `user_id` and
   `chat_id`, and hands back a URL behind an inline button. Ten-minute expiry.
2. `GET /link?t=…` redeems that token — once, ever — mints `state` and a PKCE
   verifier, carries the ticket onto a second row keyed by `state`, and redirects.
   **The bot's token stops here**: it is not carried into the browser's history
   or into whatever `Referer` the authorization server sees.
3. `GET /link/callback?code&state` redeems the `state` row, exchanges the code
   with the verifier, asks the issuer's `userinfo` endpoint who it was, and
   writes the identity row — with the `user_id` from the ticket. Nothing in the
   request is read for it.

A callback carrying a `state` nobody issued is refused *before* the code is
spent. A `state` is single-use, so a replayed callback is refused too. The
callback row is keyed by the `state` and a cookie the redirect set together, so a
`state` read out of a Referer header, a proxy log or a browser history addresses
nothing on its own.

**A bind link is honoured only in a private chat**: it is a bearer credential
for one account, and in a group the first member to open it would bind their
own Telegram id to the sender's account. Sender resolution decides who may drive the bot, not who can read
what it posts.

Binding is asymmetric: one Telegram account may hold several identities, but an
identity names exactly one Telegram account. The conditional write refuses a
second tenant claiming an identity, and re-linking to the same one succeeds so a
flow that died after the write can be retried.

The two ticket rows carry `expires_at`, the table's TTL attribute. TTL deletion
lags by up to 48 hours, so it is a sweeper: redemption checks the expiry itself.

🔴 Both new row shapes share the bots table, and two readers walk it without a
sort-key condition — `find_by_user_id` and `find_all`. A shape they do not
recognise is not skipped; it is parsed as a bot, fails, and takes the whole read
with it. Adding a row shape under a `user_id#` partition means teaching
`find_by_user_id` about it (see `is_identity_row`), and `find_all` takes only
partitions it can name rather than excluding the shapes known today.
`tests/identity_link_test.rs` holds both.

### Turning it on

Register the redirect URI (`terraform output link_redirect_uri`) with the
authorization server, then:

```bash
# terraform.tfvars — requires mcp_issuer
#   link_client_id = "<the client id>"
terraform -chdir=terraform/envs/dev apply \
  -target=aws_ssm_parameter.link_client_secret \
  -target=module.lambda_mcp_http \
  -target=aws_iam_role_policy.mcp_http_app \
  -target=aws_ssm_parameter.telebot_base_env
aws ssm put-parameter --overwrite --type SecureString \
  --name /scalable-cluster/dev/mcp/link-client-secret --value "<the client secret>"
```

Then `telebot-deploy`, which is what puts `APP__LINK__URL` on the bot. Until it
has one, the button says linking is not set up rather than producing a dead end.

### Unbinding

`/unlink` in the bot releases the caller's own Telegram id, so another can be
bound from the web; the `workos` identity has no release path, because it is
the account. Releasing is scoped to the caller's own links by a condition on
the delete, so it is not a way to take an identity off someone else.

A link written by hand needs both rows, or the listing will not see it: the
identity row above, and the tenant's own listing —
`pk = user_id#<user id>`, `sk = identity#<provider>#<subject>`.

Note that enabling the table's TTL is part of this and touches the live bots
table. It is safe by inspection — no other row shape carries `expires_at` — but
it is a change to that table, so apply it deliberately.
