# PBTB Console

The browser console for the bot manager: a Vite + React + TypeScript single-page
app served from GitHub Pages at `https://iengai.github.io/pbtb-rust/`. It drives
the REST surface in [`docs/web-api.md`](../docs/web-api.md) and embeds the
per-bot return curves the daily collector publishes.

No UI kit and no chart library: the charts are the dependency-free SVG code of
the original return-curve page, moved into `src/chart/`.

## Layout

```
site/
  index.html            Vite entry
  src/
    main.tsx            router (basename = BASE_URL) + auth provider
    App.tsx             routes
    auth/               PKCE + OAuth (public client), session in sessionStorage
    api/                /api/v1 client, response types, load/action hooks, dev mock
    chart/              return curve (windowing, re-basing, SVG) + backtest equity chart
    components/         nav, pills/badges/tiles, banners, modal, icons
    data/               static JSON access (data/, templates/)
    pages/              Login, Callback, Bots, BotDetail, AddBot, Configs, ConfigDetail, Account, Returns
  templates/            strategy-template backtests (committed; see below)
  data/                 per-bot return series (gitignored; CI syncs it from S3)
  dist/                 build output (gitignored)
```

## Scripts

| Command | What |
| --- | --- |
| `npm run dev` | dev server on `http://localhost:5173` (base `/`) |
| `npm run build` | `tsc --noEmit` + `vite build` → `dist/` (base `/pbtb-rust/`) |
| `npm run preview` | serve `dist/` on `http://localhost:4173/pbtb-rust/` |
| `npm run lint` | `tsc --noEmit` + `eslint .` |

`.claude/skills/verify/scripts/gate.sh` runs `lint` and `build` whenever
`site/package.json` exists.

## Configuration

Build-time env, read via `import.meta.env` (copy `.env.example` to `.env`):

| Variable | Meaning |
| --- | --- |
| `VITE_API_URL` | The Lambda Function URL, **with its trailing slash**. Also sent as the OAuth `resource`; the token's `aud` must equal it byte for byte. |
| `VITE_OAUTH_ISSUER` | The AuthKit issuer; the app uses `/oauth2/authorize` and `/oauth2/token` on it. |
| `VITE_OAUTH_CLIENT_ID` | The public Connect application. Registered redirect URIs: `http://localhost:5173/callback`, `https://iengai.github.io/pbtb-rust/callback`. |
| `VITE_EGRESS_IP` | Optional. The NAT egress IP shown on the Account page and in the add-bot hint. |
| `VITE_MOCK_API` | Dev only. `1` replaces the API with an in-memory fake and a fake session (`src/api/mock.ts`), for working on pages without an issuer. Not in production bundles. |

All three required values are public identifiers, not secrets; the pages
workflow sets them inline.

## Auth

OAuth 2.1 authorization code + PKCE (S256, WebCrypto) as a public client —
hand-rolled in `src/auth/oauth.ts`; the AuthKit JS SDK is not used because it
talks to a different endpoint and mints a token for a different audience.
`state` and the code verifier live in `sessionStorage` for the round trip; the
access token lives in memory + `sessionStorage` and is dropped on the first
401. It is only good for five minutes (`accessTokenExpiry` on the AuthKit
application), so the login asks for `offline_access` too and the API client
renews the token from the refresh token whenever it is within a minute of
expiring — one exchange at a time, since WorkOS rotates the refresh token on
every use. Signing out, a revoked session or a spent refresh token is what
ends the session, not the clock. A 403 without an `error=` code (a verified
subject nobody has linked from the Telegram bot) shows the "not linked" login
variant; a 403 with
`insufficient_scope` asks for a fresh sign-in with both scopes.

`/configs` and `/configs/:name` render without a token (static data). Every
other page requires one; `/bots/:id` polls the API every 15 s.

## Static data: `data/` and `templates/`

Both directories sit at the project root, not under `public/`, because the
pages workflow's S3 sync path is `site/data/` and must stay so. A small plugin
in `vite.config.ts` makes them behave like `public/`: served verbatim by `vite
dev` and `vite preview`, copied verbatim into `dist/` after `vite build`. The
build also copies `index.html` to `dist/404.html` so GitHub Pages resolves deep
links through the SPA router.

- `data/index.json` — `[{id, name}]`; `data/<id>.json` — a `BotReturnSeries`
  (`points[{ts, index, return_pct}]`, `config_switches[{ts, template_name}]`,
  `capital_resets[ts]`). The id is an opaque hash; the console finds a bot's
  chart by matching the API's bot `name` against `index.json`.
- `templates/index.json` — `[{name, engine, exchange, coins, start, end,
  metrics}]`; `templates/<name>.json` — the same plus `starting_balance`,
  `strategies[{name, side}]`, `points[{ts, equity, balance}]` normalized to
  100 at the backtest start, and the `source_sha` / `generated_at` the
  pipeline uses to skip unchanged templates. `metrics` follow passivbot's
  `analysis.json` (`gain` is the final/starting ratio, `adg*` and
  `drawdown_worst` are fractions). **Never put strategy parameters in these
  files**, and that includes the template's `description`: the authors' notes
  name leverage, position counts and exposure caps, so the description is
  deliberately absent here and reaches a signed-in tenant only through
  `GET /api/v1/templates/{name}`.

`templates/` is generated, committed output of `scripts/backtest_templates.py`
(its docstring has the flags). The script syncs the `predefined/` templates
from S3 and reruns each one through the passivbot engine its name selects
(`-v810` → the `passivbot` checkout at v8.1.0, everything else → the `pb-v712`
worktree at v7.12.0, both siblings of this repo with their own venv). A
template is reproducible from its own file — window, coins and exchange are
inside it — which is why the pipeline reruns rather than mining passivbot's
`backtests/` directory, whose runs are named by pid and timestamp with no link
back to a config. It is CPU-bound and runs on a developer machine; a template
whose artifact already carries the same `source_sha` and engine is skipped, so
a rerun after adding or editing templates only costs the changed ones. Commit
the resulting JSON.

Four of the `xrp` templates come close to liquidation inside their
window (`backtest_completion_ratio < 1`); the pages badge them rather than
headline the pre-wipe gain.

## Deploy

`.github/workflows/pages-publish.yml`: sync S3 → `site/data/`, `npm ci && npm
run build` in `site/`, upload `site/dist`. Daily, or `gh workflow run
pages-publish.yml --ref main`.

GitHub Pages is enough: the app is static, the OAuth redirect URI is just a
path on the Pages origin, the API sits on the Lambda with CORS, and the only
values in the bundle are public identifiers. The repository being public does
not matter for the same reason. What would justify moving is a private site
(pointless while the API does the authentication), a custom domain (Pages
supports one), or server rendering (nothing needs it). If one of those ever
applies, an S3 + CloudFront origin in the same Terraform is the replacement;
the front end does not change.
