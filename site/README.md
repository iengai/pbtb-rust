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
401. A 403 without an `error=` code (a verified subject nobody has linked from
the Telegram bot) shows the "not linked" login variant; a 403 with
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
  description, metrics}]`; `templates/<name>.json` — the same plus
  `strategies[{name, side}]` and `points[{ts, equity, balance}]` normalized to
  100 at the backtest start. `metrics` follow passivbot's `analysis.json`
  (`gain` is the final/starting ratio, `adg*` and `drawdown_worst` are
  fractions). The description is the author's free text and is rendered
  verbatim. **Never put strategy parameters in these files.**

The two template files in the repo are placeholder fixtures until the backtest
pipeline (`docs/web-frontend-plan.md` §3) publishes the real set.

## Deploy

`.github/workflows/pages-publish.yml`: sync S3 → `site/data/`, `npm ci && npm
run build` in `site/`, upload `site/dist`. Daily, or `gh workflow run
pages-publish.yml --ref main`.
