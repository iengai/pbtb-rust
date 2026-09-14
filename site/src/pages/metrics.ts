// Backtest metrics as passivbot's analysis.json reports them: `gain` is the
// final/starting balance ratio, the drawdowns and `adg*` are fractions, the
// ratios are raw. Formats for the keys the pages show, in display order; a key
// the list does not know is rounded to three decimals. The labels live in the
// catalog under `t.configs.metric`, keyed by the same metric key.

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
