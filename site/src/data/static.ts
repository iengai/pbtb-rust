// The static JSON published beside the app, none of it tenant data and none
// of it behind a token: `templates/`, the backtests of the strategy templates
// (committed), and `data/`, the showcase the daily collector writes for the
// operator's public bots (synced from the chart bucket by pages-publish;
// absent in a checkout that never ran `npm run fixtures`). A signed-in
// account's own return curves are not here: they come through the API.

import type { EquityPoint } from "../chart/equitySvg";

const BASE = import.meta.env.BASE_URL;

export type TemplateSummary = {
  name: string;
  title?: string;
  title_zh?: string;
  style?: string | null;
  generation?: number | null;
  engine: string;
  exchange: string;
  coins: string[];
  start: string;
  end: string;
  description: string;
  metrics: Record<string, number>;
};

export type TemplateBacktest = TemplateSummary & {
  points: EquityPoint[];
  strategies: { name: string; side: string }[];
};

// One showcase bot in `data/index.json`: an opaque id (the collector's hash,
// stable across renames), the copy-trading page it links to, and the last 30
// daily returns as a sparkline.
export type ShowcaseEntry = {
  id: string;
  name: string;
  exchange: string;
  public_url: string;
  current_return_pct: number;
  spark: number[];
};

export type ShowcaseIndex = { generated_at: number; bots: ShowcaseEntry[] };

// `data/bots/{id}.json`: the curve as a return index per day, the config
// switches and the capital resets, each with the capital the bot ran at
// rounded to a magnitude. Percentages only; no balance and no realized figure.
export type ShowcasePoint = { ts: number; index: number; return_pct: number };
export type ShowcaseSwitch = { ts: number; template_name: string; cap_usdt: number };
export type ShowcaseReset = { ts: number; cap_usdt: number };
export type ShowcaseBot = {
  id: string;
  name: string;
  exchange: string;
  public_url: string;
  generated_at: number;
  current_return_pct: number;
  points: ShowcasePoint[];
  config_switches: ShowcaseSwitch[];
  capital_resets: ShowcaseReset[];
};

async function getJSON<T>(path: string): Promise<T> {
  const r = await fetch(`${BASE}${path}`, { cache: "no-cache" });
  if (!r.ok) throw new Error(`${path} -> HTTP ${r.status}`);
  return r.json() as Promise<T>;
}

// A file the publisher may legitimately not have written: no showcase yet, or
// a bot id that is not (or no longer) public. Pages answers 404 for those,
// which to a page is "nothing here", not an error.
async function getJSONOrNull<T>(path: string): Promise<T | null> {
  const r = await fetch(`${BASE}${path}`, { cache: "no-cache" });
  if (r.status === 404) return null;
  if (!r.ok) throw new Error(`${path} -> HTTP ${r.status}`);
  return r.json() as Promise<T>;
}

export const staticData = {
  templates: () => getJSON<TemplateSummary[]>("templates/index.json"),
  template: (name: string) => getJSON<TemplateBacktest>(`templates/${encodeURIComponent(name)}.json`),
  showcase: () => getJSONOrNull<ShowcaseIndex>("data/index.json"),
  showcaseBot: (id: string) => getJSONOrNull<ShowcaseBot>(`data/bots/${encodeURIComponent(id)}.json`),
  // Every showcase bot's full series, for the config page's live runs. A bot
  // listed in the index whose file is missing (a publish caught mid-run) is
  // left out rather than failing the page.
  showcaseBots: async (): Promise<ShowcaseBot[]> => {
    const index = await getJSONOrNull<ShowcaseIndex>("data/index.json");
    if (!index) return [];
    const bots = await Promise.all(
      index.bots.map((b) => getJSONOrNull<ShowcaseBot>(`data/bots/${encodeURIComponent(b.id)}.json`)),
    );
    return bots.filter((b): b is ShowcaseBot => b != null);
  },
};
