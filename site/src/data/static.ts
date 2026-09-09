// The static JSON published beside the app: `data/` (per-bot return series,
// synced from S3 by pages-publish) and `templates/` (backtests of the strategy
// templates). Neither needs a token.

import { type BotReturnSeries, type IndexEntry, normalizeIndex } from "../chart/returnCurve";
import type { EquityPoint } from "../chart/equitySvg";

const BASE = import.meta.env.BASE_URL;

export type TemplateSummary = {
  name: string;
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

async function getJSON<T>(path: string): Promise<T> {
  const r = await fetch(`${BASE}${path}`, { cache: "no-cache" });
  if (!r.ok) throw new Error(`${path} -> HTTP ${r.status}`);
  return r.json() as Promise<T>;
}

export const staticData = {
  chartIndex: async (): Promise<IndexEntry[]> => normalizeIndex(await getJSON<unknown>("data/index.json")),
  chart: (id: string) => getJSON<BotReturnSeries>(`data/${encodeURIComponent(id)}.json`),
  templates: () => getJSON<TemplateSummary[]>("templates/index.json"),
  template: (name: string) => getJSON<TemplateBacktest>(`templates/${encodeURIComponent(name)}.json`),
};

// The return chart is keyed by an opaque id; the API knows the bot by name.
// The published index carries both, so a bot finds its chart through its name.
export function chartIdFor(index: IndexEntry[], botName: string): string | null {
  return index.find((e) => e.name === botName)?.id ?? null;
}
