# Web API

The REST surface the browser console drives. `src/interface/api/` is a sibling
of `src/interface/mcp/` and `src/interface/telegram/`: all three adapters call
the same use cases, so the exclusive start lock and the tenant boundary hold for
a `fetch` from a browser exactly as they do for a tool call or a button press.

It is served by the same Lambda as the MCP surface (`mcp_http`), under
`/api/v1`, behind the same bearer check. One token, one host, one way of being
turned away.

## Why it exists next to MCP

Three things the web needs that the MCP surface must not, or need not, offer:

- **Key entry.** Adding a bot means entering exchange keys. A tool argument lands
  in a model's context and a transcript, so `add_bot` is absent from MCP by
  design; a browser posting over TLS has no such audience. `POST /bots` is the
  one route on any surface that accepts a secret. The body is handed to the use
  case and nothing of it is logged or echoed.
- **Signing up.** `POST /signup` creates the account a principal is resolved
  from, so it is the one route that answers a subject with no account. A tool
  cannot be reached before that account exists.
- **JSON in, JSON out.** A tool result is text a model reads; a page wants
  status codes and structured bodies.

Everything else has a tool beside it ([mcp.md](mcp.md)), rendered by the same
functions (`src/interface/describe.rs`), so the two surfaces answer with one
shape.

## What it never returns

🔴 **A strategy's parameters.** A template's config is what makes a bot worth
running, and a caller who can read it can run it anywhere. No route returns a
config: templates and bots are *described* — name, description, sides, coins,
risk level, leverage, engine line — and the `bot` section stays on the server.
`tests/web_api.rs` asserts on the responses rather than trusting review.

Note that the MCP tool `get_bot_config` does return the stored config. It is
the one call on any surface that does, and it sits behind a scope of its own
(`config:read`, [mcp.md](mcp.md)) that a signed-up user's token does not carry.
Keep it that way; do not add a config route here.

**Exchange keys**, as everywhere: `api_key` / `secret_key` are never part of a
response.

## Authentication

Identical to MCP: `Authorization: Bearer <token>`, verified by the same
`TokenVerifier` — with an issuer configured, a WorkOS access token audienced for
this function's URL whose subject has an account here (`POST /signup` is what
creates one) and whose account is active. The tenant is the token's; no route
takes a `user_id`, and a bot outside the caller's tenant answers exactly like a
bot that does not exist (404).

`POST /signup` is the one route that takes a verified subject *without* an
account: it creates the account (level 0, active) and writes the `workos`
identity, or answers with the account the subject already has. It is explicit
so that authenticating with Google is never, by itself, an account. The
shared-bearer transport names no subject and cannot sign anyone up (403).

| Refusal | Status | `WWW-Authenticate` |
| --- | --- | --- |
| no token, or one that does not verify | 401 | `error="invalid_token"` + `resource_metadata` |
| a token whose subject has no account (sign up first), or a suspended one | 403 | `resource_metadata` only |
| a good token without the scope a route needs | 403 | `error="insufficient_scope", scope="bots:write"` |

Scopes are two of the three MCP defines: `bots:read` for every `GET`,
`bots:write` for everything else. No route here asks for `config:read`.

The browser gets its token with the OAuth authorization-code flow + PKCE
against the issuer, as a **public** client (no secret), with
`resource=<this function's URL>` so the token's `aud` is this server. The client id and redirect URIs live with the issuer, not here: see
[workos.md](workos.md), including the environment-level CORS list the token
endpoint needs before a browser can complete a sign-in.

## Routes

All under `/api/v1`. Bodies and responses are JSON; every response carries
`Cache-Control: no-store`.

| Route | Scope | Telegram equivalent | Notes |
| --- | --- | --- | --- |
| `POST /signup` | — | — | verified subject in, account out: 201 `created` / 200 `existing`, `{user_id, vip_level}` |
| `GET /me` | read | — | `{user_id, vip_level, scopes, telegram, identities:[{provider, subject}]}`; `telegram` is the bound Telegram id or `null` |
| `POST /me/telegram/bind-ticket` | write | — | `{token, url, expires_in}`: a one-time `/start` payload for the bot; `url` is the `https://t.me/<bot>?start=` deep link when `APP__TELEGRAM__BOT_USERNAME` is set |
| `DELETE /me/telegram` | write | `/unlink` | releases the caller's bound Telegram id so another can be bound; the `workos` identity has no release route |
| `GET /bots` | read | List | `{bots:[…]}`, each with observed `phase` |
| `POST /bots` | write | Add bot | `{name, api_key, secret_key, overwrite?}` → 201 `added`; 409 `already_exists` unless `overwrite: true` (then 200 `overwritten`: keys rotated, id and desired state kept) |
| `GET /bots/{id}` | read | State | bot + observed runtime + `config` described (or `null`) |
| `DELETE /bots/{id}` | write | Delete API key | body `{confirm: "<id>"}`; drops the row, the config and the keys |
| `POST /bots/{id}/start` | write | Run bot | claims the start lock; `started` / `already_running` / `already_starting`; 409 `stopping` (retry); 403 `quota_exceeded` past the level's ceiling |
| `POST /bots/{id}/stop` | write | Stop bot | `stopped` / `not_running` / `already_stopping`; 409 `start_in_progress` (retry) |
| `PUT /bots/{id}/risk` | write | Risk level | `{long, short}` wallet exposure limits; out-of-range → 400 |
| `PUT /bots/{id}/sides` | write | Sides | `{side: "long"\|"short", enabled}` |
| `PUT /bots/{id}/runtime` | write | Runtime | `{runtime: "py"\|"rs"}` |
| `POST /bots/{id}/template` | write | Choose config | `{name}`, applies on the next start; 403 `insufficient_level` for a template above the caller's level |
| `GET /bots/{id}/balance` | read | Balance | 501: a placeholder in telebot, so a placeholder here |
| `POST /bots/{id}/unstuck` | write | Unstuck | 501, likewise |
| `GET /bots/{id}/returns` | read | — | the bot's return series as the daily collector wrote it (a normalized index, no balances); 404 until it has; 501 where no chart bucket is configured |
| `GET /templates` | read | Choose config | `{templates:[{name, min_vip_level}]}`; nothing is hidden by level |
| `GET /templates/{name}` | read | — | the template described (with `min_vip_level`), never its parameters |

A template is addressed by its id and read by its `title` / `title_zh`, which
every described config carries alongside `template_name`. `GET /templates`
lists ids only; the console joins them with the titles in the published
backtests it already loads.

Config changes take effect on a bot's next start. A running task keeps the
config and the binary it started with.

Balance and unstuck are placeholders in telebot too (`$0.00` / "coming soon"),
so the API mirrors them rather than inventing behaviour. A real balance has an
infrastructure cost, not a code cost: the exchange keys are IP-whitelisted to
the NAT's elastic IP, and `mcp_http` runs outside the VPC with no NAT egress,
so it cannot reach the exchange at all. Doing it means moving the function into
the VPC behind the NAT, which is its own piece of work.

Errors: `400 {error}` for the caller's own input (including the validation
errors a use case raises, verbatim), `403 {error, message, …}` for what the
account's level does not allow (`quota_exceeded {limit}`,
`insufficient_level {required, current}` — `error` is a code, `message` the
same in words; the site keeps the session on these, unlike the token refusals
above), `404 {error:"not found"}`, `409 {status,…}` for a write the bot's state
does not admit right now, and `500`/`503 {error, retryable}` for a use-case
fault — redacted the way a chat reply is, to a category and a correlation id,
with the cause logged under that id.

## Levels

An account's `vip_level` (0–9, `GET /me`) sets what it may do; the table is
`src/domain/entitlement.rs`. Level 0 runs one bot at a time, each level adds
one, level 9 is unlimited; the count is over bots switched on (desired state),
so a bot that is on but between tasks holds its slot, and starting a bot that
is already on never trips it. A template may ask for a level
(`pbtb.min_vip_level` in its JSON, absent = 0): applying one above the caller's
is refused, listing and describing are not. Levels are changed with
`python scripts/ops/pbtb_ops.py set-vip`; lowering one stops nothing, it
only refuses the next start past the new ceiling.

Every write is logged with principal, route, bot id and outcome.

## CORS

The Function URL carries the CORS policy (`web_origins` in
`terraform/envs/dev`); AWS answers the preflight at the edge and the function
never sees an `OPTIONS`. The allowed origin is the console's, nothing wider. No
credentials flag: the token is a header, never a cookie.

## Running it

Nothing beyond `mcp_http`: the surface is on by construction whenever that
function is. Code updates go through
`gh workflow run lambda-deploy.yml --ref main -f target=mcp-http`; the CORS
policy is a scoped apply of `aws_lambda_function_url.mcp_http`.

Verify without credentials:

```bash
curl -i "$URL/api/v1/bots"                       # 401 + WWW-Authenticate
curl -i -X OPTIONS "$URL/api/v1/bots" \
  -H 'Origin: https://iengai.github.io' -H 'Access-Control-Request-Method: GET'   # 200 + allow-origin
```
