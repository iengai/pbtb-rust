// The per-bot return series written by the daily_pnl_snapshot Lambda, and the
// windowing, re-basing and SVG drawing that turn it into the return chart.
// Data lives under ./data/: index.json (list of bots) + <bot_id>.json (a
// BotReturnSeries). The series is normalized — a time-weighted return index and
// cumulative return %, no absolute balances. No chart library.

export type DailyPoint = { ts: number; index: number; return_pct?: number };
export type SwitchMarker = { ts: number; template_name: string };
export type BotReturnSeries = {
  id?: string;
  name?: string;
  exchange?: string;
  generated_at?: number;
  current_return_pct?: number;
  points?: DailyPoint[];
  config_switches?: SwitchMarker[];
  capital_resets?: number[];
};
export type IndexEntry = { id: string; name: string };
export type ViewPoint = { ts: number; return_pct: number };

export const fmtPct = (v: number): string => (Number.isFinite(v) ? `${v.toFixed(2)}%` : "—");
export const fmtSignedPct = (v: number): string =>
  Number.isFinite(v) ? `${v > 0 ? "+" : ""}${v.toFixed(2)}%` : "—";
export const fmtDate = (sec: number): string => new Date(sec * 1000).toISOString().slice(0, 10);
const MONTHS = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
// Axis labels: "Jun 12" — the year is implied by the window.
const fmtAxisDate = (sec: number): string => {
  const d = new Date(sec * 1000);
  return `${MONTHS[d.getUTCMonth()]} ${d.getUTCDate()}`;
};

// An account can decay to an index of (effectively) zero — a real liquidation.
// Re-basing a window onto such a start has no meaningful denominator: 0/0, or a
// percentage measured against a stake that no longer exists. Report the state
// rather than a number that reads as performance.
const DEAD_EPS = 1e-9; // at or below this the account is wiped out

// Preset look-back windows, stock-chart style. `days: null` means all history.
export const RANGES: { k: string; days: number | null }[] = [
  { k: "30D", days: 30 },
  { k: "90D", days: 90 },
  { k: "180D", days: 180 },
];
export const DEFAULT_RANGE = 1; // 90D

// index.json is a list of { id, name }. Tolerate a legacy bare-string array (id
// used as its own name) so an old published index still renders.
export function normalizeIndex(idx: unknown): IndexEntry[] {
  const arr: unknown[] = Array.isArray(idx)
    ? idx
    : ((idx as { bots?: unknown[] } | null)?.bots ?? []);
  return arr
    .map((e) => (typeof e === "string" ? { id: e, name: e } : (e as IndexEntry)))
    .filter((e) => e && e.id);
}

export type WindowStats = {
  label: string;
  ret: number;
  peak: number;
  maxDrawdown: number;
  days: number;
};

export type ChartWindow =
  | {
      kind: "ok";
      view: ViewPoint[];
      switches: SwitchMarker[];
      reset: number | null;
      stats: WindowStats;
      footer: string;
    }
  | { kind: "empty"; message: string; hint?: string };

// The selected look-back window of a series, re-based to its first point (0%
// at the window start), so every preset reads as "return over this period" —
// the way a stock chart shows 1M / 3M / 1Y.
export function selectWindow(s: BotReturnSeries, rangeI: number): ChartWindow {
  const all = (s.points || []).slice().sort((a, b) => a.ts - b.ts);

  if (all.length < 2) {
    return { kind: "empty", message: "Not enough data to plot yet." };
  }

  const range = RANGES[rangeI] ?? RANGES[DEFAULT_RANGE]!;
  const lastPoint = all[all.length - 1]!;
  const lastTs = lastPoint.ts;
  let win = all;
  if (range.days != null) {
    const cutoff = lastTs - range.days * 86400;
    win = all.filter((p) => p.ts >= cutoff);
    if (win.length < 2) win = all.slice(-2); // window shorter than history: show what we have
  }

  // A capital reset separates two unrelated stakes: the account was wiped out
  // and re-funded, and the index restarts at 100 there. Re-basing across one
  // would report the deposit as a return, so the window shows the era it ends
  // in — never a curve stitched from before and after.
  const winStart = win[0]!.ts;
  const reset = (s.capital_resets || []).filter((t) => t >= winStart && t <= lastTs).pop();
  if (reset != null) win = win.filter((p) => p.ts >= reset);

  if (win.length < 2) {
    return {
      kind: "empty",
      message: `The account was re-funded on ${fmtDate(reset!)} — not enough data since then to plot.`,
      hint: "Everything before that date was earned on capital that no longer exists.",
    };
  }

  // Re-base the cumulative index to the window start.
  const base = win[0]!.index;
  const label = reset != null ? "Since re-funding" : range.days == null ? "Total" : range.k;

  // Nothing left to measure against: the account was already at zero when this
  // window opened. Any percentage here would be invented.
  if (!(base > DEAD_EPS)) {
    return {
      kind: "empty",
      message: `Account was already at zero when this ${label} window opened — no return to compute.`,
      hint: "The capital was lost earlier in this bot's history.",
    };
  }

  const view: ViewPoint[] = win.map((p) => ({ ts: p.ts, return_pct: (p.index / base - 1) * 100 }));

  const last = view[view.length - 1]!;
  const peak = view.reduce((m, p) => Math.max(m, p.return_pct), -Infinity);
  // Worst peak-to-trough fall of the re-based index inside the window.
  let runMax = -Infinity;
  let maxDrawdown = 0;
  for (const p of view) {
    const idx = 1 + p.return_pct / 100;
    if (idx > runMax) runMax = idx;
    const dd = (idx / runMax - 1) * 100;
    if (dd < maxDrawdown) maxDrawdown = dd;
  }

  const era =
    reset != null
      ? ` · index restarted ${fmtDate(reset)}, when the wiped account was re-funded`
      : "";
  const footer = s.generated_at
    ? `${s.exchange || "bybit"} · ${view.length} days shown · time-weighted, deposit-adjusted${era} · updated ${fmtDate(s.generated_at)} UTC`
    : "";

  const switches = (s.config_switches || []).filter(
    (c) => c.ts >= view[0]!.ts && c.ts <= last.ts,
  );

  return {
    kind: "ok",
    view,
    switches,
    reset: reset ?? null,
    stats: { label, ret: last.return_pct, peak, maxDrawdown, days: view.length },
    footer,
  };
}

// --- SVG line chart (cumulative return %, config-switch markers) ---

const W = 900,
  H = 300,
  M = { l: 56, r: 12, t: 14, b: 30 };
const PW = W - M.l - M.r,
  PH = H - M.t - M.b;

type Scales = {
  x: (t: number) => number;
  y: (v: number) => number;
  t0: number;
  t1: number;
  vmin: number;
  vmax: number;
};

function scales(pts: ViewPoint[]): Scales {
  const t0 = pts[0]!.ts,
    t1 = pts[pts.length - 1]!.ts;
  let vmin = 0,
    vmax = 0; // always include the 0% baseline
  for (const p of pts) {
    if (p.return_pct < vmin) vmin = p.return_pct;
    if (p.return_pct > vmax) vmax = p.return_pct;
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
  return { x, y, t0, t1, vmin, vmax };
}

function linePath(pts: ViewPoint[], sc: Scales): string {
  return pts
    .map((p, i) => `${i ? "L" : "M"}${sc.x(p.ts).toFixed(1)} ${sc.y(p.return_pct).toFixed(1)}`)
    .join(" ");
}

// Return % at an arbitrary ts, linearly interpolated between daily points, so a
// config-switch marker sits exactly on the curve.
function returnAt(ts: number, pts: ViewPoint[]): number {
  const n = pts.length;
  if (ts <= pts[0]!.ts) return pts[0]!.return_pct;
  if (ts >= pts[n - 1]!.ts) return pts[n - 1]!.return_pct;
  for (let i = 1; i < n; i++) {
    if (pts[i]!.ts >= ts) {
      const a = pts[i - 1]!,
        b = pts[i]!;
      const f = (ts - a.ts) / (b.ts - a.ts || 1);
      return a.return_pct + f * (b.return_pct - a.return_pct);
    }
  }
  return pts[n - 1]!.return_pct;
}

export function chartSVG(pts: ViewPoint[], switches: SwitchMarker[]): string {
  const sc = scales(pts);
  const up = pts[pts.length - 1]!.return_pct >= 0;

  // Horizontal grid + y labels (percent).
  let grid = "";
  const ROWS = 5;
  for (let i = 0; i <= ROWS; i++) {
    const v = sc.vmin + ((sc.vmax - sc.vmin) * i) / ROWS;
    const y = sc.y(v).toFixed(1);
    grid += `<line x1="${M.l}" y1="${y}" x2="${W - M.r}" y2="${y}" stroke="var(--grid)"/>`;
    grid += `<text x="${M.l - 8}" y="${y}" text-anchor="end" dominant-baseline="middle" fill="var(--muted)" font-size="11">${v > 0 ? "+" : ""}${v.toFixed(1)}%</text>`;
  }

  // Emphasized 0% baseline.
  const zeroY = sc.y(0).toFixed(1);
  const baseline = `<line x1="${M.l}" y1="${zeroY}" x2="${W - M.r}" y2="${zeroY}" stroke="var(--muted)" stroke-width="1" opacity="0.5"/>`;

  // X labels.
  let xlab = "";
  const COLS = 5;
  for (let i = 0; i <= COLS; i++) {
    const t = sc.t0 + ((sc.t1 - sc.t0) * i) / COLS;
    xlab += `<text x="${sc.x(t).toFixed(1)}" y="${H - 8}" text-anchor="${i === 0 ? "start" : i === COLS ? "end" : "middle"}" fill="var(--muted)" font-size="11">${fmtAxisDate(t)}</text>`;
  }

  // Config-switch markers: a dot sitting ON the return curve at each switch,
  // with a native hover tooltip naming the config it switched to.
  let sw = "";
  for (const c of switches) {
    if (c.ts < sc.t0 || c.ts > sc.t1) continue;
    const x = sc.x(c.ts).toFixed(1);
    const y = sc.y(returnAt(c.ts, pts)).toFixed(1);
    sw += `<circle cx="${x}" cy="${y}" r="5" fill="var(--switch)" stroke="var(--panel)" stroke-width="2"><title>→ ${escapeXml(c.template_name)} · ${fmtDate(c.ts)}</title></circle>`;
  }

  const color = up ? "var(--pnl)" : "var(--pnl-neg)";
  const path = linePath(pts, sc);
  const bottom = (M.t + PH).toFixed(1);
  const area = `<path d="${path} L${sc.x(sc.t1).toFixed(1)} ${bottom} L${sc.x(sc.t0).toFixed(1)} ${bottom} Z" fill="${color}" opacity="0.08"/>`;
  const line = `<path d="${path}" fill="none" stroke="${color}" stroke-width="2" stroke-linejoin="round"/>`;

  return `<svg viewBox="0 0 ${W} ${H}" role="img" aria-label="return curve">
    ${grid}${baseline}${xlab}${area}${sw}${line}
    <line data-part="cursor" x1="0" y1="${M.t}" x2="0" y2="${M.t + PH}" stroke="var(--accent)" stroke-width="1" opacity="0"/>
    <circle data-part="dot" r="3.5" fill="${color}" opacity="0"/>
    <rect data-part="hit" x="${M.l}" y="${M.t}" width="${PW}" height="${PH}" fill="transparent"/>
  </svg>`;
}

// Hover: a cursor line + dot snapped to the nearest daily point, and a fixed
// tooltip beside the pointer. Returns the teardown.
export function wireHover(container: HTMLElement, tip: HTMLElement, pts: ViewPoint[]): () => void {
  const svg = container.querySelector("svg");
  const hit = container.querySelector<SVGElement>('[data-part="hit"]');
  const cursor = container.querySelector<SVGElement>('[data-part="cursor"]');
  const dot = container.querySelector<SVGElement>('[data-part="dot"]');
  if (!svg || !hit || !cursor || !dot) return () => {};
  const sc = scales(pts);

  const show = (on: boolean) => {
    for (const el of [cursor, dot]) el.setAttribute("opacity", on ? "1" : "0");
    tip.style.opacity = on ? "1" : "0";
  };

  const move = (e: MouseEvent) => {
    const rect = svg.getBoundingClientRect();
    const vx = ((e.clientX - rect.left) / rect.width) * W;
    let best = 0,
      bd = Infinity;
    for (let i = 0; i < pts.length; i++) {
      const d = Math.abs(sc.x(pts[i]!.ts) - vx);
      if (d < bd) {
        bd = d;
        best = i;
      }
    }
    const p = pts[best]!;
    const px = sc.x(p.ts);
    cursor.setAttribute("x1", String(px));
    cursor.setAttribute("x2", String(px));
    dot.setAttribute("cx", String(px));
    dot.setAttribute("cy", String(sc.y(p.return_pct)));
    show(true);
    tip.innerHTML =
      `<div class="d">${fmtDate(p.ts)}</div>` +
      `<div class="row"><span>Return</span><b>${fmtPct(p.return_pct)}</b></div>`;
    tip.style.left = Math.min(e.clientX + 14, window.innerWidth - 150) + "px";
    tip.style.top = e.clientY + 14 + "px";
  };
  const leave = () => show(false);

  hit.addEventListener("mousemove", move);
  hit.addEventListener("mouseleave", leave);
  return () => {
    hit.removeEventListener("mousemove", move);
    hit.removeEventListener("mouseleave", leave);
    show(false);
  };
}

// A trend glyph for a listing row: the window's curve squeezed into a small
// box, no axes.
export function sparklinePoints(
  pts: { ts: number; v: number }[],
  w: number,
  h: number,
  pad = 2,
): string {
  if (pts.length < 2) return "";
  const t0 = pts[0]!.ts,
    t1 = pts[pts.length - 1]!.ts;
  let vmin = Infinity,
    vmax = -Infinity;
  for (const p of pts) {
    if (p.v < vmin) vmin = p.v;
    if (p.v > vmax) vmax = p.v;
  }
  if (vmin === vmax) {
    vmin -= 1;
    vmax += 1;
  }
  return pts
    .map((p) => {
      const x = ((p.ts - t0) / (t1 - t0 || 1)) * w;
      const y = pad + (1 - (p.v - vmin) / (vmax - vmin)) * (h - 2 * pad);
      return `${x.toFixed(1)},${y.toFixed(1)}`;
    })
    .join(" ");
}

export function escapeXml(s: unknown): string {
  return String(s).replace(/[<>&]/g, (c) => ({ "<": "&lt;", ">": "&gt;", "&": "&amp;" })[c]!);
}
