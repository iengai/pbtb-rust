# PBTB Console

The browser console for the bot manager: a Vite + React + TypeScript single-page
app served from GitHub Pages at `https://iengai.github.io/pbtb-rust/`. It drives
the REST surface in [`docs/web-api.md`](../docs/web-api.md), which is also where
the per-bot return curves come from: each account sees its own.

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
    chart/              return curve (windowing, re-basing, SVG), the showcase's runs, the config page's
                        combined chart (backtest + live runs over a chosen period, presets and a brush)
    components/         nav, pills/badges/tiles, banners, modal, icons
    data/               static JSON access (templates/, data/)
    i18n/               English + Simplified Chinese catalogs (see Languages below)
    pages/              Login, Callback, Bots, BotDetail, AddBot, Configs, ConfigDetail (+ ConfigChart), Account,
                        Returns, Showcase, ShowcaseBot
  templates/            strategy-template backtests (committed; see below)
  dist/                 build output (gitignored)
```

## Languages

English and Simplified Chinese, hand-rolled (no i18n library), in `src/i18n/`:

```
i18n/
  locale.tsx      Lang = "en" | "zh"; detectLang/loadLang/saveLang; LocaleProvider (context
                  {lang, setLang, t}); useT() -> Messages; useLang() -> {lang, setLang}; LangSwitch
  messages.ts     Messages = typeof en; messages: Record<Lang, Messages>
  en/index.ts     en = { common, auth, bots, configs, account, returns, showcase }
  en/<area>.tsx   one file per page area, plain object literal
  zh/index.ts     zh: Messages = { common, auth, bots, configs, account, returns, showcase }
  zh/<area>.tsx   import { <area> as en } from "../en/<area>"; export const <area>: typeof en = {...}
```

Each `zh/<area>.tsx` is typed `typeof en`, so a key the zh side is missing or invents is a `tsc`
error (`npm run lint` runs `tsc --noEmit`) — parity between the two catalogs is enforced by the
compiler, not by convention. A catalog leaf is a `string`, or a typed function of its interpolated
values returning a `string` or `ReactNode` (never build a message by concatenating leaves — Chinese
word order differs from English); rich text with inline markup (`<b>`, `<span>`) is a function
returning `ReactNode`, which is why catalog files are `.tsx`. Components read `t.<area>.<key>` via
`useT()`.

The pure modules — `chart/returnCurve.ts`, `chart/showcase.ts`, `chart/combo.ts`, `pages/metrics.ts` — stay
language-free: they return structured data (an enum-like reason, numbers, timestamps) rather than
sentences, and the React side (`ReturnChart.tsx`, `ComboChart.tsx`, the pages) resolves it through
`t`.

The language is stored in `localStorage["pbtb.lang"]`; absent that, it defaults from
`navigator.languages` (the first entry starting with `zh` picks `"zh"`, else `"en"`).

## Scripts

| Command | What |
| --- | --- |
| `npm run dev` | dev server on `http://localhost:5173` (base `/`) |
| `npm run build` | `tsc --noEmit` + `vite build` → `dist/` (base `/pbtb-rust/`) |
| `npm run preview` | serve `dist/` on `http://localhost:4173/pbtb-rust/` |
| `npm run lint` | `tsc --noEmit` + `eslint .` |
| `npm test` | vitest, the unit tests beside the pure modules (`src/**/*.test.ts`) |

`.claude/skills/verify/scripts/gate.sh` runs `lint`, `test` and `build` whenever
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
`state` and the code verifier live in `sessionStorage` for the round trip —
they belong to one tab and one redirect. A 403 without an `error=` code (a
verified subject nobody has linked from the Telegram bot) shows the "not
linked" login variant; a 403 with `insufficient_scope` asks for a fresh
sign-in with both scopes.

### The session

An access token is good for five minutes (`accessTokenExpiry` on the AuthKit
application), so the login asks for `offline_access` too and the API client
renews the token from the refresh token whenever it is within a minute of
expiring. Signing out, a revoked session, or a spent refresh token is what
ends a session — not the clock, and not closing the tab: it is kept in
`localStorage`. WorkOS rotates the refresh token on every use, so only one
exchange may run at a time; `navigator.locks` serializes the tabs, and the tab
that gets in second re-reads the session and finds it already renewed.

The stricter answer — an HttpOnly, SameSite cookie the JavaScript cannot read
— needs a backend on the site's own domain to hold the token and proxy the
API. The API is a Lambda Function URL on `amazonaws.com`, so its cookies are
third-party to `github.io` and Safari and Firefox drop them outright; the
alternative is not a cookie flag but the custom domain + CloudFront migration
described under **Deploy**, plus a Lambda that keeps the refresh token instead
of verifying a JWT. What is left to lose to XSS is bounded by what the token
is: five minutes, `aud` fixed to this API, `bots:read`/`bots:write`, and a
refresh token that rotates — and an XSS on this origin could call the API as
the user with or without a readable token. The bundle carries no third-party
script, and the two `innerHTML` sites are the return chart and its tooltip,
which interpolate numbers and `escapeXml` every string that comes from data
(labels, titles, the `data-href` attribute); the config page's chart is JSX.

`/configs`, `/configs/:name`, `/p` and `/p/bots/:id` render without a token (static
data). Every other page requires one; `/bots/:id` polls the API every 15 s. Return curves
(`/returns`, and the chart on `/bots/:id`) come from `GET
/api/v1/bots/{id}/returns` — a `BotReturnSeries` (`points[{ts, index,
return_pct}]`, `config_switches[{ts, template_name}]`, `capital_resets[ts]`)
the daily collector writes under the owner's tenant, so an account only ever
sees its own bots' curves. A 404 there means the collector has not written the
bot yet.

## Static data: `data/` (the showcase)

Synced from the chart bucket's `public/` prefix by `pages-publish` (daily at
01:40 UTC and on every publish), never committed; `npm run fixtures` copies
the sample in `fixtures/data/` into place, so a local build serves it. Two shapes,
written by the daily collector (`src/bin/daily_pnl_snapshot/model.rs`,
`PublicIndex` / `PublicBotSeries`):

- `data/index.json`: `{generated_at, bots[{id, name, exchange, public_url,
  current_return_pct, spark[30 × return_pct]}]}`.
- `data/bots/{id}.json`: `{id, name, exchange, public_url, generated_at,
  current_return_pct, points[{ts, index, return_pct}], config_switches[{ts,
  template_name, cap_usdt}], capital_resets[{ts, cap_usdt}]}`.

`id` is opaque (twelve hex characters), not the bot id. Percentages only:
`cap_usdt`, the capital the bot ran a config at, is the one balance-derived
figure and is rounded to one significant digit; there is no realized
PnL and no balance. `capital_resets` dates a wipe-out-and-refund. The S3
layout is in docs/data-model.md.

## Static data: `templates/`

The directory sits at the project root, not under `public/`, beside the script
that generates it. A small plugin in `vite.config.ts` makes it behave like
`public/`: served verbatim by `vite dev` and `vite preview`, copied verbatim
into `dist/` after `vite build`. The build also copies `index.html` to
`dist/404.html` so GitHub Pages resolves deep links through the SPA router.

- `templates/index.json` — `[{name, title, title_zh, style, generation,
  engine, audience, exchange, coins, start, end, metrics}]`, where `name` is the
  opaque template id and `audience` is `"operator"` on a template offered to
  the operator's account only (the Configs list leaves it out unless `GET /me`
  says the session is the operator's; its page stays reachable by link); `templates/<name>.json` — the same plus `starting_balance`,
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
from S3 and reruns each one through the passivbot engine its `config_version`
selects (v8 → the `passivbot` checkout at v8.1.0, v7 → the `pb-v712` worktree
at v7.12.0, both siblings of this repo with their own venv). A
template is reproducible from its own file — window, coins and exchange are
inside it — which is why the pipeline reruns rather than mining passivbot's
`backtests/` directory, whose runs are named by pid and timestamp with no link
back to a config. It is CPU-bound and runs on a developer machine; a template
whose artifact already carries the same `source_sha`, engine and window end is
skipped, so a rerun after adding or editing templates only costs the changed
ones. `--end-date now` runs every template from its own start to the last
complete day (the template in S3 is not touched; the artifact's `end` says
which), which is how the catalogue is brought up to date; without the flag a
template keeps the end its artifact was run to, and only a template with no
artifact yet runs to its own `backtest.end_date`. The descriptions quote the
artifact, so run `describe_templates.py --apply` after. Commit the resulting
JSON.

The `extreme`-profile templates come close to liquidation inside their window
(`backtest_completion_ratio < 1`); the pages badge them rather than headline the
pre-wipe gain.

The config page draws the backtest's equity (balance dashed) and the live runs
of that template on the showcase bots (`chart/showcase.ts`) on one chart,
`chart/combo.ts` + `chart/ComboChart.tsx`. Every curve
arrives as an index and is re-based to 0% at its first point inside the chosen
period, so a backtest and a run over the same days read as returns over those
days; the period comes from the presets (anchored at the latest point any
curve reaches) or the brush strip under the chart. The newest run is drawn
first; the run list under the chart ticks the others on.

## Deploy

`.github/workflows/pages-publish.yml`: `npm ci && npm run build` in `site/`,
upload `site/dist`. On every push to `main` that touches `site/`, or `gh
workflow run pages-publish.yml --ref main`. No cloud credentials: the page is
code and the committed backtests, nothing per tenant.

GitHub Pages is enough: the app is static, the OAuth redirect URI is just a
path on the Pages origin, the API sits on the Lambda with CORS, and the only
values in the bundle are public identifiers. The repository being public does
not matter for the same reason. What would justify moving is a private site
(pointless while the API does the authentication), a custom domain (Pages
supports one), or server rendering (nothing needs it). If one of those ever
applies, an S3 + CloudFront origin in the same Terraform is the replacement;
the front end does not change.
