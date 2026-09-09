// The static JSON published beside the app: `templates/`, the backtests of the
// strategy templates. Public by design — a backtest is not tenant data — and
// needs no token. Return curves are not here: they are one account's own and
// come through the API.

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
  templates: () => getJSON<TemplateSummary[]>("templates/index.json"),
  template: (name: string) => getJSON<TemplateBacktest>(`templates/${encodeURIComponent(name)}.json`),
};
