# PBTB Console

The browser console for the bot manager: a Vite + React + TypeScript single-page
app served from GitHub Pages at `https://iengai.github.io/pbtb-rust/`. It drives
the REST surface in [`docs/web-api.md`](../docs/web-api.md), which is also where
the per-bot return curves come from: each account sees its own.

No UI kit and no chart library: the charts are the dependency-free SVG code of
the original return-curve page, moved into `src/chart/`.

The pages work down to phone width, with one breakpoint at 760px in
`src/styles.css`: the header's links fold onto a row of their own that scrolls
sideways, and the grids drop to one or two columns. A chart is drawn at its
container's measured width, one viewBox unit to a CSS pixel (`chart/frame.ts`,
`chart/useWidth.ts`), so its text does not shrink on a small screen. It reads
by touch as well as by mouse.

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
    data/               static JSON access (templates/, the showcase)
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
| `VITE_SHOWCASE_URL` | Optional. The showcase CDN, **with its trailing slash**; it answers CORS for the Pages origin alone. Unset, the showcase pages read `data/` beside the app (see Static data below). |
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
data). A `/configs` card names the bots that hold its template now: the
showcase's, by each public file's latest switch, for everyone but the operator
(whose own bots they are), and with a token the account's own, from `GET /bots`
and one `GET /bots/{id}` per bot. Every other page requires a token; `/bots/:id` polls the API every 15 s. Return curves
(`/returns`, and the chart on `/bots/:id`) come from `GET
/api/v1/bots/{id}/returns` — a `BotReturnSeries` (`points[{ts, index,
return_pct}]`, `config_switches[{ts, template_name}]`, `capital_resets[ts]`)
the daily collector writes under the owner's tenant, so an account only ever
sees its own bots' curves. A 404 there means the collector has not written the
bot yet.

## Static data: the showcase

Read at run time from `VITE_SHOWCASE_URL`, the showcase CDN over the chart
bucket's `public/` prefix, never built into the site. Unset, as in a local
build, the pages read `data/` beside the app instead, which `npm run fixtures`
fills from the sample in `fixtures/data/`. Two shapes, built in
`src/domain/showcase.rs` (`PublicIndex` / `PublicBotSeries`):

- `index.json`: `{generated_at, bots[{id, name, exchange, public_url,
  current_return_pct, spark[30 × return_pct]}]}`.
- `bots/{id}.json`: `{id, name, exchange, public_url, generated_at,
  current_return_pct, points[{ts, index, return_pct}], config_switches[{ts,
  template_name, cap_usdt}], capital_resets[{ts, cap_usdt}]}`.

`public_url` is `null` on a bot shown without a Bybit link. The pages link it
only when it is an https URL on bybit.com (`bybitLink` in `src/data/static.ts`)
and leave the link out otherwise.

`id` is opaque (twelve hex characters), not the bot id. Percentages only:
`cap_usdt`, the capital the bot ran a config at, is the one balance-derived
figure and is rounded to one significant digit; there is no realized
PnL and no balance. `capital_resets` dates a wipe-out-and-refund. The S3
layout is in docs/data-model.md.

Which bots are shown is the operator's choice on `/p/manage` (`GET
/api/v1/showcase`, `PUT /api/v1/bots/{id}/showcase`), reachable from the
showcase page when `GET /me` says the session is the operator's. A switch
writes or removes the bot's file and rebuilds `index.json` at once, and the
CDN serves the change within about half a minute; a shown bot with no
collected curve yet (`published: false`) appears after the collector's next
run. How the prefix is kept is in docs/data-model.md.

## Static data: `templates/`

The directory sits at the project root, not under `public/`, beside the script
that generates it. A small plugin in `vite.config.ts` makes it behave like
`public/`: served verbatim by `vite dev` and `vite preview`, copied verbatim
into `dist/` after `vite build`. The build also copies `index.html` to
`dist/404.html` so GitHub Pages resolves deep links through the SPA router.

- `templates/index.json` — `[{name, title, title_zh, style, generation,
  engine, audience, exchange, coins, start, end, starting_balance, params_sha,
  metrics}]`, where `name` is the opaque template id, `params_sha` is the sha of
  the strategy without its `backtest` block (templates that carry the same one
  are one parameter set at several capitals, which a card and a template's page
  name; it skips no backtest) and `audience` is `"operator"` on a retired template,
  offered to the operator's account only (the Configs page lists it under its
  Retired tab when `GET /me` says the session is the operator's and leaves it
  out otherwise; its page stays reachable by link). The `audience` here is the
  snapshot's: everyone but the operator reads it through the overlay
  `templates/audience.json` beside the showcase (`VITE_SHOWCASE_URL`,
  `{generated_at, published[id]}`; `staticData.templatesPublished`), where a
  template the overlay does not name is retired, and the snapshot's mark holds
  only while there is no overlay (none written, an error, or no answer in 3 s;
  locally, with no `VITE_SHOWCASE_URL`, always). For the operator both pages
  read the audience live from `GET /api/v1/templates` instead, and a template's
  page offers Retire / Publish (`PUT /api/v1/templates/{name}/audience`), which
  rewrites the overlay; the public pages follow within about half a minute.
  How the overlay is kept is in docs/data-model.md. `templates/<name>.json` — the same plus
  `strategies[{name, side}]`, `points[{ts, equity, balance}]` normalized to
  100 at the backtest start, and the `source_sha` / `trading_sha` /
  `generated_at` the pipeline uses to skip unchanged templates (`trading_sha`
  covers what passivbot reads alone, so an audience switch does not re-run a
  backtest). `metrics` follow passivbot's
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
whose artifact already carries the same `source_sha`, engine, window end and
candle directory (`ohlcv_source_dir`, the one its run read) is skipped, so a
rerun after adding or editing templates only costs the changed ones.
`--end-date now` runs every template from its own start to the last complete
day (the template in S3 is not touched; the artifact's `end` says which),
which is how the catalogue is brought up to date; without the flag a template
keeps the end its artifact was run to, and only a template with no artifact
yet runs to its own `backtest.end_date`. The mainstream templates read candles
from `caches/ohlcv_padded` (their `backtest.ohlcv_source_dir`), whose alts end
2025-10-28 and BTC and XRP 2025-11-18; the XRP templates that set no directory
read passivbot's own data. A window past the padded data needs
`--ohlcv-source-dir` naming a directory that covers it (on the developer
machine `caches/ohlcv_combined`, kept current by the strategy lab's
`fetch_live_ohlcv.py`); the flag applies to every template, the XRP ones
included. When a run has a candle directory, from the template or the flag,
and that directory stops before the day before the window end, the template
fails instead of running: passivbot completes such a run without the missing
candles and reports the window as complete. A template with no candle
directory is not checked. The descriptions quote the artifact, so run
`describe_templates.py --apply` after. Commit the resulting JSON.

The `extreme`-profile templates come close to liquidation inside their window
(`backtest_completion_ratio < 1`); the pages badge them rather than headline the
pre-wipe gain.

The config page draws the backtest's equity (balance dashed) and the live runs
of that template on the showcase bots (`chart/showcase.ts`) on one chart,
`chart/combo.ts` + `chart/ComboChart.tsx`. Every curve
arrives as an index and is re-based to 0% at its first point inside the chosen
period, so a backtest and a run over the same days read as returns over those
days; the period comes from the presets (anchored at the latest point any
curve reaches) or the brush strip under the chart. The y-axis is log by
default (rows at equal multiples of the period's first point, labelled in %,
0% always in view and padded below only where a curve goes under it) with a
Linear toggle. The bot with the newest run is drawn first, all its spans on
the config in its colour; the bot list under the chart ticks the others on,
and "untick all" clears them.

## Deploy

`.github/workflows/pages-publish.yml`: `npm ci && npm run build` in `site/`,
upload `site/dist`. On every push to `main` that touches `site/`, or `gh
workflow run pages-publish.yml --ref main`. No cloud credentials: the page is
code and the committed backtests, nothing per tenant; the showcase comes from
the CDN at run time, so neither a switch nor a collector run needs a publish.

GitHub Pages is enough: the app is static, the OAuth redirect URI is just a
path on the Pages origin, the API sits on the Lambda with CORS, and the only
values in the bundle are public identifiers. The repository being public does
not matter for the same reason. What would justify moving is a private site
(pointless while the API does the authentication), a custom domain (Pages
supports one), or server rendering (nothing needs it). If one of those ever
applies, an S3 + CloudFront origin in the same Terraform is the replacement;
the front end does not change.
