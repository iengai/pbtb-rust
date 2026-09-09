# Web API

The REST surface the browser console drives. `src/interface/api/` is a sibling
of `src/interface/mcp/` and `src/interface/telegram/`: all three adapters call
the same use cases, so the exclusive start lock and the tenant boundary hold for
a `fetch` from a browser exactly as they do for a tool call or a button press.

It is served by the same Lambda as the MCP surface (`mcp_http`), under
`/api/v1`, behind the same bearer check. One token, one host, one way of being
turned away.

## Why it exists next to MCP

Two things the web needs that the MCP surface must not, or need not, offer:

- **Key entry.** Adding a bot means entering exchange keys. A tool argument lands
  in a model's context and a transcript, so `add_bot` is absent from MCP by
  design; a browser posting over TLS has no such audience. `POST /bots` is the
  one route on any surface that accepts a secret. The body is handed to the use
  case and nothing of it is logged or echoed.
- **JSON in, JSON out.** A tool result is text a model reads; a page wants
  status codes and structured bodies.

## What it never returns

🔴 **A strategy's parameters.** A template's config is what makes a bot worth
running, and a caller who can read it can run it anywhere. No route returns a
config: templates and bots are *described* — name, description, sides, coins,
risk level, leverage, engine line — and the `bot` section stays on the server.
`tests/web_api.rs` asserts on the responses rather than trusting review.

Note that the MCP tool `get_bot_config` does return the stored config. That
surface serves one operator (`APP__MCP__USER_ID`, on the allowlist), not the
public. Keep it that way; do not add a config route here.

**Exchange keys**, as everywhere: `api_key` / `secret_key` are never part of a
response.

## Authentication

Identical to MCP: `Authorization: Bearer <token>`, verified by the same
`TokenVerifier` — with an issuer configured, a WorkOS access token audienced for
this function's URL whose subject has been linked from the Telegram bot and
whose linked account is on the allowlist. The tenant is the token's; no route
takes a `user_id`, and a bot outside the caller's tenant answers exactly like a
bot that does not exist (404).

| Refusal | Status | `WWW-Authenticate` |
| --- | --- | --- |
| no token, or one that does not verify | 401 | `error="invalid_token"` + `resource_metadata` |
| a token whose subject is not linked, or not allowlisted | 403 | `resource_metadata` only |
| a good token without the scope a route needs | 403 | `error="insufficient_scope", scope="bots:write"` |

Scopes are the two MCP defines: `bots:read` for every `GET`, `bots:write` for
everything else.

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
| `GET /me` | read | Link account | `{user_id, scopes, identities:[{provider, subject}]}` |
| `DELETE /me/identities` | write | `/unlink` | releases every identity linked to the caller, including the one this request used |
| `GET /bots` | read | List | `{bots:[…]}`, each with observed `phase` |
| `POST /bots` | write | Add bot | `{name, api_key, secret_key, overwrite?}` → 201 `added`; 409 `already_exists` unless `overwrite: true` (then 200 `overwritten`: keys rotated, id and desired state kept) |
| `GET /bots/{id}` | read | State | bot + observed runtime + `config` described (or `null`) |
| `DELETE /bots/{id}` | write | Delete API key | body `{confirm: "<id>"}`; drops the row, the config and the keys |
| `POST /bots/{id}/start` | write | Run bot | claims the start lock; `started` / `already_running` / `already_starting`; 409 `stopping` (retry) |
| `POST /bots/{id}/stop` | write | Stop bot | `stopped` / `not_running` / `already_stopping`; 409 `start_in_progress` (retry) |
| `PUT /bots/{id}/risk` | write | Risk level | `{long, short}` wallet exposure limits; out-of-range → 400 |
| `PUT /bots/{id}/sides` | write | Sides | `{side: "long"\|"short", enabled}` |
| `PUT /bots/{id}/runtime` | write | Runtime | `{runtime: "py"\|"rs"}` |
| `POST /bots/{id}/template` | write | Choose config | `{name}`, applies on the next start |
| `GET /bots/{id}/balance` | read | Balance | 501: a placeholder in telebot, so a placeholder here |
| `POST /bots/{id}/unstuck` | write | Unstuck | 501, likewise |
| `GET /templates` | read | Choose config | `{templates:[names]}` |
| `GET /templates/{name}` | read | — | the template described, never its parameters |

Config changes take effect on a bot's next start. A running task keeps the
config and the binary it started with.

Errors: `400 {error}` for the caller's own input (including the validation
errors a use case raises, verbatim), `404 {error:"not found"}`, `409 {status,…}`
for a write the bot's state does not admit right now, and `500`/`503
{error, retryable}` for a use-case fault — redacted the way a chat reply is, to a
category and a correlation id, with the cause logged under that id.

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
