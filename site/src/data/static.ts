// The static JSON the pages read without a token, none of it tenant data:
// `templates/`, the backtests of the strategy templates (committed, published
// beside the app), and from `VITE_SHOWCASE_URL` (the showcase CDN over the
// chart bucket's `public/` prefix) the showcase, the operator's shown bots,
// and `templates/audience.json`, the templates published now. Unset, both are
// read from `data/` beside the app, where `npm run fixtures` copies the
// showcase sample and no overlay. A signed-in account's own return curves are
// not here: they come through the API.

const BASE = import.meta.env.BASE_URL;
const SHOWCASE = (import.meta.env.VITE_SHOWCASE_URL || `${BASE}data/`).replace(/\/?$/, "/");

// One row of a backtest curve: equity and balance, each normalized to 100 at
// the backtest start.
export type EquityPoint = { ts: number; equity: number; balance: number };

export type TemplateSummary = {
  name: string;
  title?: string;
  title_zh?: string;
  style?: string | null;
  generation?: number | null;
  /** Whether it holds one position at a time or several: the list's first split. Null on a
   *  template that trades no side. */
  positions?: "single" | "multi" | null;
  engine: string;
  /** `operator`: offered to the operator's account only; the list leaves it out for anyone else,
   *  its page stays reachable by link. Absent or null: everyone. */
  audience?: "operator" | null;
  exchange: string;
  coins: string[];
  start: string;
  end: string;
  /** The balance the backtest started with: the least capital the template is offered for. */
  starting_balance?: number | null;
  description: string;
  metrics: Record<string, number>;
};

export type TemplateBacktest = TemplateSummary & {
  points: EquityPoint[];
  strategies: { name: string; side: string }[];
};

// One showcase bot in the showcase `index.json`: an opaque id (the collector's
// hash, stable across renames), the copy-trading page it links to (null on a
// bot shown without one), and the last 30 daily returns as a sparkline.
export type ShowcaseEntry = {
  id: string;
  name: string;
  exchange: string;
  public_url: string | null;
  current_return_pct: number;
  spark: number[];
};

export type ShowcaseIndex = { generated_at: number; bots: ShowcaseEntry[] };

// The showcase `bots/{id}.json`: the curve as a return index per day, the
// config switches and the capital resets, each with the capital the bot ran at
// rounded to a magnitude. Percentages only; no balance and no realized figure.
export type ShowcasePoint = { ts: number; index: number; return_pct: number };
export type ShowcaseSwitch = { ts: number; template_name: string; cap_usdt: number };
export type ShowcaseReset = { ts: number; cap_usdt: number };
export type ShowcaseBot = {
  id: string;
  name: string;
  exchange: string;
  public_url: string | null;
  generated_at: number;
  current_return_pct: number;
  points: ShowcasePoint[];
  config_switches: ShowcaseSwitch[];
  capital_resets: ShowcaseReset[];
};

// A bot's copy-trading link as a page may put it in an href: an https URL on
// bybit.com, or null. The link is typed in by the operator, and an href runs
// whatever scheme it is given.
export function bybitLink(url: string | null | undefined): string | null {
  if (!url) return null;
  let parsed: URL;
  try {
    parsed = new URL(url);
  } catch {
    return null;
  }
  const host = parsed.hostname.toLowerCase();
  const onBybit = host === "bybit.com" || host.endsWith(".bybit.com");
  return parsed.protocol === "https:" && onBybit ? parsed.href : null;
}

function withBybitLink<T extends { public_url: string | null }>(item: T): T {
  return { ...item, public_url: bybitLink(item.public_url) };
}

async function getJSON<T>(url: string): Promise<T> {
  const r = await fetch(url, { cache: "no-cache" });
  if (!r.ok) throw new Error(`${url} -> HTTP ${r.status}`);
  return r.json() as Promise<T>;
}

// A file the publisher may legitimately not have written: no showcase yet, or
// a bot id that is not (or no longer) public. The host answers 404 for those,
// which to a page is "nothing here", not an error.
async function getJSONOrNull<T>(url: string): Promise<T | null> {
  const r = await fetch(url, { cache: "no-cache" });
  if (r.status === 404) return null;
  if (!r.ok) throw new Error(`${url} -> HTTP ${r.status}`);
  return r.json() as Promise<T>;
}

async function showcaseIndex(): Promise<ShowcaseIndex | null> {
  const index = await getJSONOrNull<ShowcaseIndex>(`${SHOWCASE}index.json`);
  return index && { ...index, bots: index.bots.map(withBybitLink) };
}

async function showcaseBot(id: string): Promise<ShowcaseBot | null> {
  const bot = await getJSONOrNull<ShowcaseBot>(`${SHOWCASE}bots/${encodeURIComponent(id)}.json`);
  return bot && withBybitLink(bot);
}

// The ids in the showcase CDN's `templates/audience.json`, the overlay the
// template switch and the template scripts rewrite: a set, or null when the
// value is not `{published: string[]}`.
export function parsePublished(value: unknown): Set<string> | null {
  if (typeof value !== "object" || value === null) return null;
  const published = (value as { published?: unknown }).published;
  if (!Array.isArray(published) || !published.every((id) => typeof id === "string")) return null;
  return new Set(published);
}

// Whether a template is retired: by the overlay when there is one, which names
// every published template, and by the committed snapshot's own mark when
// there is none.
export function isRetired(tpl: { name: string; audience?: "operator" | null }, published: Set<string> | null): boolean {
  return published ? !published.has(tpl.name) : tpl.audience === "operator";
}

// How long the catalogue waits on the showcase edge (the overlay, the showcase
// bots) before it draws without it: from the snapshot's audience marks, and
// with no showcase row.
export const SLOW_EDGE_MS = 3000;

async function templatesPublished(): Promise<Set<string> | null> {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), SLOW_EDGE_MS);
  try {
    const r = await fetch(`${SHOWCASE}templates/audience.json`, { cache: "no-cache", signal: controller.signal });
    if (!r.ok) return null;
    return parsePublished(await r.json());
  } catch {
    return null;
  } finally {
    clearTimeout(timer);
  }
}

export const staticData = {
  templates: () => getJSON<TemplateSummary[]>(`${BASE}templates/index.json`),
  templatesPublished,
  template: (name: string) => getJSON<TemplateBacktest>(`${BASE}templates/${encodeURIComponent(name)}.json`),
  showcase: showcaseIndex,
  showcaseBot,
  // Every showcase bot's full series, for the config page's live runs. A bot
  // listed in the index whose file is missing (a publish caught mid-run) is
  // left out rather than failing the page.
  showcaseBots: async (): Promise<ShowcaseBot[]> => {
    const index = await showcaseIndex();
    if (!index) return [];
    const bots = await Promise.all(index.bots.map((b) => showcaseBot(b.id)));
    return bots.filter((b): b is ShowcaseBot => b != null);
  },
};
