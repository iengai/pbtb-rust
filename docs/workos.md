# WorkOS (AuthKit)

WorkOS is the OAuth issuer for everything that reaches this system from
outside Telegram: the MCP endpoint, the account-linking flow, and the web
console. One AuthKit environment plays three roles at once — authorization
server for the MCP surface (tokens audienced for our Function URL), identity
provider for `Link account` (the `sub` it hands back is what
`identity#workos#<sub>` records), and login for the browser console. The
system-side halves are in [mcp.md](mcp.md) (verification, linking) and
[web-api.md](web-api.md) (the REST surface the console calls); this page is
the WorkOS-side configuration, what depends on each piece, and the gotchas
that cost time.

Everything here is reachable through the WorkOS dashboard or the WorkOS MCP
server in Claude Code (`whoami` → `list_operations` → `query` / `mutate`).
The operation names below are that server's.

## Environments

| | Staging (**live today**) | Production |
| --- | --- | --- |
| environment id | `environment_01M208X0Z1ET1008YJT62WS5M6` | `environment_01M208X1J7NZYA6HTM257E6YEP` |
| issuer | `https://growing-starlight-99-staging.authkit.app` | not activated (`productionState = Inactive`) |
| AuthKit application client id | `client_01M208X1BKTZP0NXNK5THCJHFD` | `client_01M208X1QJN89HRRNJ7X2WAGA8` |

Everything below lives in Staging. Moving to Production means recreating every
object in the checklist at the end and changing one line of tfvars plus three
build-time variables.

## Objects, and what depends on each

### Permissions — the OAuth scopes

| slug | id | meaning |
| --- | --- | --- |
| `bots:read` | `permission_01M221PCDBDFH9GCXK82TMX6KF` | list bots, read status and described config |
| `bots:write` | `permission_01M221PK4BMQ38V0F3E320GFHZ` | start/stop, change config fields, add/delete, unlink |
| `config:read` | `(create it: see checklist)` | read a bot's stored passivbot config, strategy parameters included |

`config:read` belongs to the **MCP** application only. The web console never
returns a config, so assigning it there would hand every browser sign-in the
one thing that surface exists to withhold.

A permission's **slug is the OAuth scope**. A Connect application may request
exactly the permissions assigned to it (`setApplicationPermissions`); a token
that asked for none of them carries no scope and is refused by the first tool
or route it reaches (no read floor, by design). No JWT template is involved.
Where a token also carries a `permissions` claim — what the user's role holds —
our side takes the intersection with `scope` ([mcp.md](mcp.md)).

### Connect application `pbtb-rust MCP` — confidential

| | |
| --- | --- |
| id / client id | `app_01M221M345XAZQ98764YFD3B65` / `client_01M221M345JQQ6ASA7X68S5ZT4` |
| confidentiality | Confidential — one secret, hint `c5b35b79` |
| redirect URIs | `https://wpgsvdyyl6rb4omtrgb7ox6weu0krirm.lambda-url.ap-northeast-1.on.aws/link/callback` (default), `http://localhost:8765/callback` (manual testing) |
| permissions | `bots:read`, `bots:write`; `config:read` once it is created |

Used by the **link flow** (`src/interface/link/`) as an OAuth *client*, and by
MCP clients (Claude Code, etc.) that need a client id. Our side:
`link_client_id` in `terraform/envs/dev/terraform.tfvars`, the secret in SSM
`/scalable-cluster/dev/mcp/link-client-secret` (`put-parameter --overwrite`;
Terraform holds the parameter, never the value), and `terraform output
link_redirect_uri` is the URI registered above.

### Connect application `pbtb-rust Web` — public

| | |
| --- | --- |
| id / client id | `app_01M227S251VBVAKE7JJBVWAF00` / `client_01M227S251T5NA8GTGHWT1QAVB` |
| confidentiality | Public — no secret; PKCE (S256) |
| redirect URIs | `https://iengai.github.io/pbtb-rust/callback` (default), `http://localhost:5173/callback` (`vite dev`) |
| permissions | `bots:read`, `bots:write` |

Used by the **web console** (`site/src/auth/oauth.ts`). Our side: the
`VITE_OAUTH_CLIENT_ID` / `VITE_OAUTH_ISSUER` / `VITE_API_URL` variables in
`.github/workflows/pages-publish.yml` (and `site/.env.example` for local dev).
All three are public identifiers, not secrets.

### AuthKit OAuth resource — the token audience

| id | `authkit_oauth_resource_01M21Z50M7EQBWKAEBNQ3DHSJ4` |
| --- | --- |
| uri | `https://wpgsvdyyl6rb4omtrgb7ox6weu0krirm.lambda-url.ap-northeast-1.on.aws/` |

`setAuthkitOauthResources` registers the URIs a client may name in the
`resource` parameter (RFC 8707); the token's `aud` becomes that URI. It has to
match our `resource` **byte for byte, including the trailing slash** the
Function URL carries — `OAuthTokens` compares it as a string. The value on our
side is SSM `/scalable-cluster/dev/mcp/resource-url` (read by the function) and
`VITE_API_URL` (sent by the console).

### CORS web origins — environment-wide

`corsConfig` / `updateCorsConfig`: `["https://iengai.github.io",
"http://localhost:5173"]`.

Required for any browser client. Without the origin listed, the token
endpoint's **preflight still answers `Access-Control-Allow-Origin: *`** — it
looks fine from curl — but the real `POST /oauth2/token` response carries no
CORS header at all, so the browser reports `Failed to fetch` and the console's
callback page shows "Sign-in did not complete". This is separate from the
Function URL's own CORS policy (`web_origins` in tfvars), which governs calls
to our API, not to WorkOS.

### Google sign-in

`connectionsByType GoogleOAuth`: one credential,
`oauth_credential_01M22NTSPJ3AF8BN7NHFB6WS8Y`, `isUserlandEnabled = true`,
`clientId = null` — Staging runs on WorkOS's shared test credentials, which the
dashboard enables by default. **Production needs a Google Cloud OAuth client of
its own** (`updateOauthCredentials` with `clientId` / `clientSecret`; the
redirect URI to register with Google is the credential's `redirectUri`). There
is no API to *create* the credential; the dashboard does that.

Who a Google sign-in resolves to is still decided on our side: the `sub` must
have an account (`POST /api/v1/signup`, an explicit click on the console's
signup page), or the token is refused (403, no `error=`). Signing in with
Google is never, by itself, an account. Only Google should be enabled as an
authentication method for this environment: the console assumes a `sub` is a
Google account, and the account model has no second sign-in.

## Gotchas (each one cost a real debugging session)

- **Two kinds of application.** The *AuthKit application* (dashboard →
  Applications, marked Default; `authkitApplications`) is for AuthKit's own
  `/authorize` and the AuthKit SDKs. The *Connect application*
  (`createApplication`, `type: OAuth`; `environmentApplications`) is what
  `/oauth2/*` recognises. Sending an AuthKit client id to `/oauth2/authorize`
  302s to `/oauth2/error?error=application_not_found` with no hint that you
  used the wrong kind. Both of ours are Connect applications.
- **Do not use `@workos-inc/authkit-js` or any AuthKit SDK for the console.**
  They talk to the AuthKit application's flow and mint tokens for a different
  audience; `OAuthTokens` answers 401. The console hand-rolls PKCE against
  `/oauth2/authorize` + `/oauth2/token`.
- **`resource` is honoured only by the authorization-code flow.** The device
  code flow ignores it and signs `aud = <AuthKit client id>`, which fails our
  audience check. Never validate the MCP surface with the device flow.
- **A public client is fine.** `createApplication` with
  `clientConfidentiality: Public`; the token endpoint then accepts
  `client_id` + `code_verifier` with no secret (a bogus code answers
  `invalid_code`; the confidential app answers `unauthorized` to the same
  request, which is how to tell the two apart from curl).
- **No dynamic client registration.** `/oauth2/register` is 404 and discovery
  has no `registration_endpoint`; an MCP client expecting DCR has to be handed
  the confidential client id and secret by hand.
- **The client secret is created only in the dashboard** and shown once. The
  API can only `deleteApplicationCredential`.
- **Discovery document.** Our code reads
  `/.well-known/openid-configuration` (the link flow needs `userinfo_endpoint`,
  which the `oauth-authorization-server` document lacks). All endpoints are on
  the issuer's origin, which `OAuthTokens` insists on.
- **`setRedirectUris` takes `applicationId` *or* `environmentId`, never
  both.** The MCP server auto-fills `environmentId` when you pass
  `environment_id`; for a Connect application omit `environment_id` and pass
  `applicationId` only, or it errors "mutually exclusive".
- **The issuer is behind Cloudflare.** Python's default `urllib` user agent
  gets a 403 (error 1010); set a browser-ish `User-Agent`.
- **Access tokens last five minutes.** `accessTokenExpiry = 300` on the
  environment's default AuthKit application (`maxSessionTime` is a year,
  `inactivityTimeout` two days). A client that does not renew looks like it
  logs the user out every five minutes; the console asks for `offline_access`
  and refreshes (`site/src/auth/oauth.ts`), sending the same `resource` on the
  refresh so the new token keeps the API's audience. WorkOS rotates the
  refresh token on each use, so only one exchange may be in flight.
- **Token shape we rely on:** `sub` = the WorkOS user id (`user_…`), `scope` =
  `"bots:read bots:write email openid"` (plus `offline_access` for the
  console), `aud` = the resource URI, `iss` = the issuer. The `email` claim rides in the id token as well; the console shows it
  in the header.

## Verifying from the command line

No credentials needed for any of these.

```bash
ISS=https://growing-starlight-99-staging.authkit.app
WEB=client_01M227S251T5NA8GTGHWT1QAVB
URL=https://wpgsvdyyl6rb4omtrgb7ox6weu0krirm.lambda-url.ap-northeast-1.on.aws

# 1. the public client and its redirect URI are known: 302 to the login page,
#    not to /oauth2/error?error=application_not_found
curl -s -o /dev/null -w '%{http_code} %{redirect_url}\n' -A 'Mozilla/5.0' \
  "$ISS/oauth2/authorize?response_type=code&client_id=$WEB&redirect_uri=https%3A%2F%2Fiengai.github.io%2Fpbtb-rust%2Fcallback&scope=openid%20bots%3Aread%20bots%3Awrite&code_challenge=E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM&code_challenge_method=S256&state=x&resource=$URL%2F"

# 2. the token endpoint takes the public client without a secret (invalid_code,
#    not unauthorized) AND answers the browser's origin (the CORS header)
curl -s -i -X POST "$ISS/oauth2/token" -A 'Mozilla/5.0' -H 'Origin: https://iengai.github.io' \
  -d grant_type=authorization_code -d client_id=$WEB -d code=bogus -d code_verifier=x \
  -d redirect_uri=https://iengai.github.io/pbtb-rust/callback | grep -iE '^HTTP|access-control-allow-origin|^\{'

# 3. our side: discovery answers, the API and MCP refuse without a token
curl -s "$URL/.well-known/oauth-protected-resource"
curl -s -o /dev/null -w '%{http_code}\n' "$URL/api/v1/bots"      # 401
```

The full round trip (sign in, call `/api/v1/bots`) can only be done in a
browser with a linked account; the console at
<https://iengai.github.io/pbtb-rust/> is that test.

## Checklist: recreating this in another environment

Order matters — the resource has to exist before a client can name it, and
permissions before they can be assigned.

WorkOS side (dashboard or the MCP server):

1. `createPermission` × 3: slugs `bots:read`, `bots:write`, `config:read`.
2. `setAuthkitOauthResources`: the Function URL **with** its trailing slash
   (`terraform output mcp_http_url`).
3. `createApplication` `pbtb-rust MCP` (`type: OAuth`, Confidential);
   `setApplicationPermissions` all three; `setRedirectUris` (applicationId
   only) with `terraform output link_redirect_uri`; create the secret in the
   dashboard.
4. `createApplication` `pbtb-rust Web` (`type: OAuth`, **Public**);
   `setApplicationPermissions` `bots:read` and `bots:write` only — never
   `config:read`; `setRedirectUris` with
   `https://iengai.github.io/pbtb-rust/callback` and
   `http://localhost:5173/callback`.
5. `updateCorsConfig`: `https://iengai.github.io`, `http://localhost:5173`.
6. Google: dashboard → Authentication → Google OAuth; in Production supply a
   real Google client id/secret.

Our side:

7. `terraform.tfvars`: `mcp_issuer = "<issuer>"`, `link_client_id = "<MCP
   client id>"`, `web_origins = ["https://iengai.github.io"]`; scoped apply per
   [mcp.md](mcp.md) ("Turning on per-user OAuth" / "Linking an account"); then
   `aws ssm put-parameter --overwrite --type SecureString --name
   /scalable-cluster/dev/mcp/link-client-secret --value "<secret>"` and a
   `telebot-deploy` (it carries `APP__LINK__URL`).
8. `pages-publish.yml`: `VITE_OAUTH_ISSUER`, `VITE_OAUTH_CLIENT_ID` (the
   **Web** client id), `VITE_API_URL`; `gh workflow run pages-publish.yml
   --ref main`.
9. Every user links again from the Telegram bot: subjects are per environment,
   so the `identity#workos#<sub>` rows from Staging mean nothing in Production.
10. Run the three curl checks above.
