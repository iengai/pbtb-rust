// Backtest metrics as passivbot's analysis.json reports them: `gain` is the
// final/starting balance ratio, `adg*` and `drawdown_worst` are fractions, the
// ratios are raw. Labels and formats for the ones the pages show, in display
// order; anything else in the file is shown raw after them.

export const METRICS: { key: string; label: string; fmt: (v: number) => string }[] = [
  { key: "gain", label: "Gain", fmt: (v) => fmtGain(v, 0) },
  { key: "adg", label: "ADG", fmt: (v) => `${(v * 100).toFixed(2)}%` },
  { key: "adg_w", label: "ADG (weighted)", fmt: (v) => `${(v * 100).toFixed(2)}%` },
  { key: "drawdown_worst", label: "Max drawdown", fmt: (v) => `${(v * 100).toFixed(1)}%` },
  { key: "sharpe_ratio", label: "Sharpe", fmt: (v) => v.toFixed(3) },
  { key: "sortino_ratio", label: "Sortino", fmt: (v) => v.toFixed(3) },
  { key: "calmar_ratio", label: "Calmar", fmt: (v) => v.toFixed(3) },
  { key: "positions_held_per_day", label: "Positions / day", fmt: (v) => v.toFixed(1) },
  { key: "position_held_hours_mean", label: "Position held (h, mean)", fmt: (v) => v.toFixed(1) },
  { key: "loss_profit_ratio", label: "Loss / profit", fmt: (v) => v.toFixed(2) },
  { key: "backtest_completion_ratio", label: "Window completed", fmt: (v) => `${(v * 100).toFixed(1)}%` },
];

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
  return m ? m.fmt(v) : String(v);
}

export function metricRows(metrics: Record<string, number>): { label: string; value: string }[] {
  const rows = METRICS.filter((m) => m.key in metrics).map((m) => ({
    label: m.label,
    value: m.fmt(metrics[m.key]!),
  }));
  for (const [k, v] of Object.entries(metrics)) {
    if (!METRICS.some((m) => m.key === k)) rows.push({ label: k, value: String(v) });
  }
  return rows;
}
