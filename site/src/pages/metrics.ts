// Backtest metrics as passivbot's analysis.json reports them: `gain` is the
// final/starting balance ratio; the drawdowns, `adg*`,
// `backtest_completion_ratio` and `equity_balance_diff_neg_max` are fractions;
// the other ratios are raw. Formats for the keys the pages show, in display
// order; a key the list does not know is rounded to three decimals. The labels
// live in the catalog under `t.configs.metric`, keyed by the same metric key.

export const METRICS: { key: string; fmt: (v: number) => string }[] = [
  { key: "gain", fmt: (v) => fmtGain(v, 0) },
  { key: "adg", fmt: (v) => `${(v * 100).toFixed(2)}%` },
  { key: "adg_w", fmt: (v) => `${(v * 100).toFixed(2)}%` },
  { key: "drawdown_worst", fmt: (v) => `${(v * 100).toFixed(1)}%` },
  { key: "sharpe_ratio", fmt: (v) => v.toFixed(3) },
  { key: "sortino_ratio", fmt: (v) => v.toFixed(3) },
  { key: "calmar_ratio", fmt: (v) => v.toFixed(3) },
  { key: "positions_held_per_day", fmt: (v) => v.toFixed(1) },
  { key: "position_held_hours_mean", fmt: (v) => v.toFixed(1) },
  { key: "loss_profit_ratio", fmt: (v) => v.toFixed(2) },
  { key: "backtest_completion_ratio", fmt: (v) => `${(v * 100).toFixed(1)}%` },
  { key: "drawdown_worst_mean_1pct", fmt: (v) => `${(v * 100).toFixed(1)}%` },
  { key: "omega_ratio", fmt: (v) => v.toFixed(3) },
  { key: "sterling_ratio", fmt: (v) => v.toFixed(3) },
  { key: "position_unchanged_hours_max", fmt: (v) => v.toFixed(1) },
  { key: "equity_balance_diff_neg_max", fmt: (v) => `${(v * 100).toFixed(1)}%` },
];

// A key with no entry of its own: three decimals keep the float's noise out of
// the row without pretending to a scale the catalog does not know.
function roundMetric(v: number): string {
  return String(Number(v.toFixed(3)));
}

// The engine stops a backtest when the account is liquidated, so a window that
// did not complete means a wipe-out, and the other metrics describe the run up
// to it rather than the strategy.
export function wipedOut(metrics: Record<string, number>): boolean {
  const ratio = metrics.backtest_completion_ratio;
  return ratio != null && Number.isFinite(ratio) && ratio < 1;
}

// A coin with its share of the fills, as a chip reads it: `DOGE 85%`. Every
// coin in the list was traded, so rounding may not say otherwise: a share under
// half a percent reads `<1%` rather than `0%`, and beside another coin a share
// over 99.5 reads `>99%` rather than claiming the whole run.
export function tradedLabels(traded: { coin: string; share: number }[]): string[] {
  return traded.map((c) => {
    const pct = Math.round(c.share);
    if (pct === 0) return `${c.coin} <1%`;
    if (pct === 100 && traded.length > 1) return `${c.coin} >99%`;
    return `${c.coin} ${pct}%`;
  });
}

// The strategy lab's risk bands (strategy_lab/scripts/round22_accept.py BANDS): the most
// a tier's worst drawdown reaches.
const TIER_BANDS: [number, string][] = [
  [0.12, "guard"],
  [0.2, "steady"],
  [0.28, "balanced"],
  [0.4, "bold"],
];

/** The tier a worst drawdown falls in by the lab's bands. */
export function tierOf(drawdown: number): string {
  return TIER_BANDS.find(([limit]) => drawdown <= limit)?.[1] ?? "extreme";
}

export function fmtGain(gain: number | undefined, digits: number): string {
  if (gain == null || !Number.isFinite(gain)) return "—";
  const pct = (gain - 1) * 100;
  return `${pct >= 0 ? "+" : ""}${pct.toFixed(digits)}%`;
}

export function fmtMetric(key: string, v: number | undefined): string {
  if (v == null || !Number.isFinite(v)) return "—";
  const m = METRICS.find((x) => x.key === key);
  return m ? m.fmt(v) : roundMetric(v);
}

export function metricRows(metrics: Record<string, number>): { key: string; value: string }[] {
  const rows = METRICS.filter((m) => m.key in metrics).map((m) => ({
    key: m.key,
    value: fmtMetric(m.key, metrics[m.key]),
  }));
  for (const [k, v] of Object.entries(metrics)) {
    if (!METRICS.some((m) => m.key === k)) rows.push({ key: k, value: fmtMetric(k, v) });
  }
  return rows;
}

// What the catalogue sorts by, each with the direction a reader wants first:
// the smallest capital, the largest gain, the shallowest drawdown, the highest
// Sharpe.
export const SORTS = {
  capital: { value: (tpl: Sortable) => tpl.starting_balance, desc: false },
  gain: { value: (tpl: Sortable) => tpl.metrics.gain, desc: true },
  drawdown: { value: (tpl: Sortable) => tpl.metrics.drawdown_worst, desc: false },
  sharpe: { value: (tpl: Sortable) => tpl.metrics.sharpe_ratio, desc: true },
} as const;
export type SortKey = keyof typeof SORTS;
export const SORT_KEYS = Object.keys(SORTS) as SortKey[];

type Sortable = { starting_balance?: number | null; metrics: Record<string, number> };

// A sorted copy. `flip` reverses the key's own direction. A wiped-out template
// goes last whichever way the list runs, since its metrics describe the run up
// to the wipe, and so does one without the value; ties fall back to gain.
export function sortTemplates<T extends Sortable>(list: T[], key: SortKey, flip = false): T[] {
  const { value, desc } = SORTS[key];
  const sign = desc !== flip ? -1 : 1;
  const rank = (tpl: T) => {
    const v = value(tpl);
    return wipedOut(tpl.metrics) ? 2 : v == null || !Number.isFinite(v) ? 1 : 0;
  };
  return [...list].sort(
    (a, b) =>
      rank(a) - rank(b) ||
      sign * ((value(a) ?? 0) - (value(b) ?? 0)) ||
      (b.metrics.gain ?? 0) - (a.metrics.gain ?? 0),
  );
}
