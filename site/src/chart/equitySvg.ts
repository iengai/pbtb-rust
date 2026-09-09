// The backtest equity/balance chart for a strategy template. Points are
// normalized to 100 at the backtest start; both lines are drawn as % change
// from there so the y axis reads like the return chart's.

import { escapeXml } from "./returnCurve";

export type EquityPoint = { ts: number; equity: number; balance: number };

const W = 900,
  H = 320,
  M = { l: 56, r: 12, t: 14, b: 30 };
const PW = W - M.l - M.r,
  PH = H - M.t - M.b;

const pct = (v: number) => (v / 100 - 1) * 100;
const fmtMonth = (sec: number) => new Date(sec * 1000).toISOString().slice(0, 7);

export function equitySVG(points: EquityPoint[]): string {
  const pts = points.slice().sort((a, b) => a.ts - b.ts);
  if (pts.length < 2) return "";
  const t0 = pts[0]!.ts,
    t1 = pts[pts.length - 1]!.ts;
  let vmin = 0,
    vmax = 0;
  for (const p of pts) {
    for (const v of [pct(p.equity), pct(p.balance)]) {
      if (v < vmin) vmin = v;
      if (v > vmax) vmax = v;
    }
  }
  if (vmin === vmax) {
    vmin -= 1;
    vmax += 1;
  }
  const pad = (vmax - vmin) * 0.08;
  vmin -= pad;
  vmax += pad;
  const x = (t: number) => M.l + ((t - t0) / (t1 - t0 || 1)) * PW;
  const y = (v: number) => M.t + (1 - (v - vmin) / (vmax - vmin)) * PH;

  let grid = "";
  const ROWS = 5;
  for (let i = 0; i <= ROWS; i++) {
    const v = vmin + ((vmax - vmin) * i) / ROWS;
    const yy = y(v).toFixed(1);
    grid += `<line x1="${M.l}" y1="${yy}" x2="${W - M.r}" y2="${yy}" stroke="var(--grid)"/>`;
    grid += `<text x="${M.l - 8}" y="${yy}" text-anchor="end" dominant-baseline="middle" fill="var(--muted)" font-size="11">${v > 0 ? "+" : ""}${v.toFixed(1)}%</text>`;
  }
  const zeroY = y(0).toFixed(1);
  const baseline = `<line x1="${M.l}" y1="${zeroY}" x2="${W - M.r}" y2="${zeroY}" stroke="var(--muted)" stroke-width="1" opacity="0.5"/>`;

  let xlab = "";
  const COLS = 4;
  for (let i = 0; i <= COLS; i++) {
    const t = t0 + ((t1 - t0) * i) / COLS;
    xlab += `<text x="${x(t).toFixed(1)}" y="${H - 8}" text-anchor="${i === 0 ? "start" : i === COLS ? "end" : "middle"}" fill="var(--muted)" font-size="11">${escapeXml(fmtMonth(t))}</text>`;
  }

  const path = (key: "equity" | "balance") =>
    pts
      .map((p, i) => `${i ? "L" : "M"}${x(p.ts).toFixed(1)} ${y(pct(p[key])).toFixed(1)}`)
      .join(" ");
  const eq = path("equity");
  const bottom = (M.t + PH).toFixed(1);
  const area = `<path d="${eq} L${x(t1).toFixed(1)} ${bottom} L${x(t0).toFixed(1)} ${bottom} Z" fill="var(--pnl)" opacity="0.08"/>`;
  const equity = `<path d="${eq}" fill="none" stroke="var(--pnl)" stroke-width="2" stroke-linejoin="round"/>`;
  const balance = `<path d="${path("balance")}" fill="none" stroke="var(--balance)" stroke-width="2" stroke-linejoin="round"/>`;

  return `<svg viewBox="0 0 ${W} ${H}" role="img" aria-label="backtest equity">${grid}${baseline}${xlab}${area}${equity}${balance}</svg>`;
}
