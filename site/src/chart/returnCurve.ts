// The per-bot return series written by the daily_pnl_snapshot Lambda, and the
// windowing, re-basing and SVG drawing that turn it into the return chart.
// A series (a BotReturnSeries) reaches the page through `GET
// /api/v1/bots/{id}/returns`, for the bot's owner, or as the public
// `data/bots/{id}.json` of a showcase bot. It carries a time-weighted return
// index (cumulative return %, deposit-neutral) and, for the owner, the
// realized PnL in USDT per day; never a balance. The money fields are optional
// because a series written before they existed has none and the public one
// never does. No chart library.

export type DailyPoint = {
  ts: number;
  index: number;
  return_pct?: number;
  realized_usdt?: number;
  cum_realized_usdt?: number;
};
export type SwitchMarker = { ts: number; template_name: string };
export type BotReturnSeries = {
  id?: string;
  name?: string;
  exchange?: string;
  generated_at?: number;
  current_return_pct?: number;
  total_realized_usdt?: number;
  points?: DailyPoint[];
  config_switches?: SwitchMarker[];
  capital_resets?: number[];
};
export type ViewPoint = { ts: number; return_pct: number; realized_usdt?: number };

// The stretch of a window one config was active for. The first period may
// have opened before the window did (`startsInside` false): the config was
// already running when the view begins, so its band starts at the view's edge
// and no switch dot marks it.
export type Period = { start: number; end: number; template_name: string; startsInside: boolean };

export const fmtPct = (v: number): string => (Number.isFinite(v) ? `${v.toFixed(2)}%` : "—");
export const fmtSignedPct = (v: number): string =>
  Number.isFinite(v) ? `${v > 0 ? "+" : ""}${v.toFixed(2)}%` : "—";
// Money is signed and rounded to cents; a missing figure is a dash, not a zero.
export const fmtUsdt = (v: number | null | undefined): string =>
  v != null && Number.isFinite(v)
    ? `${v > 0 ? "+" : v < 0 ? "−" : ""}${Math.abs(v).toLocaleString("en-US", { minimumFractionDigits: 2, maximumFractionDigits: 2 })} USDT`
    : "—";
export const fmtDate = (sec: number): string => new Date(sec * 1000).toISOString().slice(0, 10);

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

// What the window is measuring: the era since the account was re-funded, all
// history, or one of the presets (named by its key, "90D"). The words belong to
// the reader's language, so the module names the case and the UI says it.
export type RangeLabel =
  | { kind: "sinceRefunding" }
  | { kind: "total" }
  | { kind: "range"; k: string };

export type WindowStats = {
  label: RangeLabel;
  ret: number;
  peak: number;
  maxDrawdown: number;
  days: number;
  /** Realized PnL earned inside the window, in USDT; null when the series
   *  predates the money fields. Summed over the days after the window's
   *  first point, the same days `ret` measures. */
  pnl: number | null;
  /** Realized PnL over the bot's whole recorded history, in USDT. */
  totalPnl: number | null;
};

// The provenance line under the chart, as fields: which exchange, how many
// daily points are drawn, the re-funding this era starts at (if any), and when
// the collector last published — all timestamps in seconds.
export type WindowCaption = {
  exchange: string;
  days: number;
  resetAt: number | null;
  updatedAt: number;
};

export type ChartWindow =
  | {
      kind: "ok";
      view: ViewPoint[];
      switches: SwitchMarker[];
      periods: Period[];
      reset: number | null;
      stats: WindowStats;
      caption: WindowCaption | null;
    }
  | { kind: "empty"; reason: "noData" }
  | { kind: "empty"; reason: "resetAfterWindow"; resetAt: number }
  | { kind: "empty"; reason: "wipedOut"; label: RangeLabel };

// Worst peak-to-trough fall of a re-based curve, in percent (≤ 0).
export function maxDrawdownOf(view: ViewPoint[]): number {
  let runMax = -Infinity;
  let worst = 0;
  for (const p of view) {
    const idx = 1 + p.return_pct / 100;
    if (idx > runMax) runMax = idx;
    const dd = (idx / runMax - 1) * 100;
    if (dd < worst) worst = dd;
  }
  return worst;
}

// The selected look-back window of a series, re-based to its first point (0%
// at the window start), so every preset reads as "return over this period" —
// the way a stock chart shows 1M / 3M / 1Y.
export function selectWindow(s: BotReturnSeries, rangeI: number): ChartWindow {
  const all = (s.points || []).slice().sort((a, b) => a.ts - b.ts);

  if (all.length < 2) {
    return { kind: "empty", reason: "noData" };
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
    return { kind: "empty", reason: "resetAfterWindow", resetAt: reset! };
  }

  // Re-base the cumulative index to the window start.
  const base = win[0]!.index;
  const label: RangeLabel =
    reset != null
      ? { kind: "sinceRefunding" }
      : range.days == null
        ? { kind: "total" }
        : { kind: "range", k: range.k };

  // Nothing left to measure against: the account was already at zero when this
  // window opened. Any percentage here would be invented.
  if (!(base > DEAD_EPS)) {
    return { kind: "empty", reason: "wipedOut", label };
  }

  const view: ViewPoint[] = win.map((p) => ({
    ts: p.ts,
    return_pct: (p.index / base - 1) * 100,
    realized_usdt: p.realized_usdt,
  }));

  const last = view[view.length - 1]!;
  const earned = view.slice(1);
  const pnl = earned.every((p) => p.realized_usdt != null)
    ? earned.reduce((sum, p) => sum + p.realized_usdt!, 0)
    : null;
  const totalPnl = s.total_realized_usdt ?? null;
  const peak = view.reduce((m, p) => Math.max(m, p.return_pct), -Infinity);
  const maxDrawdown = maxDrawdownOf(view);

  // Without a collection timestamp there is nothing honest to say about how
  // fresh the curve is, so the caption is left out entirely.
  const caption: WindowCaption | null = s.generated_at
    ? {
        exchange: s.exchange || "bybit",
        days: view.length,
        resetAt: reset ?? null,
        updatedAt: s.generated_at,
      }
    : null;

  const first = view[0]!.ts;
  const sorted = (s.config_switches || []).slice().sort((a, b) => a.ts - b.ts);
  const switches = sorted.filter((c) => c.ts >= first && c.ts <= last.ts);

  // The config already running when the view opens, then every switch inside
  // it; each period runs to the next head or to the view's end.
  const active = sorted.filter((c) => c.ts <= first).pop();
  const heads = [
    ...(active ? [{ ts: first, template_name: active.template_name, startsInside: false }] : []),
    ...sorted
      .filter((c) => c.ts > first && c.ts <= last.ts)
      .map((c) => ({ ts: c.ts, template_name: c.template_name, startsInside: true })),
  ];
  const periods: Period[] = heads.map((h, i) => ({
    start: h.ts,
    end: heads[i + 1]?.ts ?? last.ts,
    template_name: h.template_name,
    startsInside: h.startsInside,
  }));

  return {
    kind: "ok",
    view,
    switches,
    periods,
    reset: reset ?? null,
    stats: { label, ret: last.return_pct, peak, maxDrawdown, days: view.length, pnl, totalPnl },
    caption,
  };
}

// --- SVG line chart (cumulative return %, config periods and switch markers) ---

// The top margin holds the period labels, so it is taller than the bottom
// one's axis ticks need.
const W = 900,
  H = 300,
  M = { l: 56, r: 12, t: 30, b: 30 };
const PW = W - M.l - M.r,
  PH = H - M.t - M.b;

// A band narrower than this gets no label: the name would be a few clipped
// glyphs, and the tooltip names the config anyway.
const LABEL_MIN_PX = 40;

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

// Every word the drawing code puts in front of a reader arrives from outside,
// already in the reader's language.
export type ChartLabels = {
  ariaLabel: string;
  /** Row name for the hovered return value in the tooltip. */
  returnRow: string;
  /** Row name for the hovered day's realized PnL, shown when the point has one. */
  pnlRow: string;
  /** Row name for the config active on the hovered day, shown when one is known. */
  configRow: string;
  /** Native `<title>` on a config-switch marker; the date is ISO. */
  switchTitle: (template: string, date: string) => string;
  /** Native `<title>` on a config period's band and label; the dates are ISO. */
  periodTitle: (template: string, from: string, to: string) => string;
  /** One x-axis tick: the year is implied by the window, so day and month only. */
  axisDate: (sec: number) => string;
};

// A period as drawn: named for the reader, and linking to the config's page
// when it has one (a retired or private template has none).
export type DrawnPeriod = Period & { label: string; href: string | null };

// `idPrefix` makes this drawing's element ids unique in the document; it
// must be stable across renders, or React replaces the subtree every time.
export function chartSVG(
  pts: ViewPoint[],
  switches: SwitchMarker[],
  periods: DrawnPeriod[],
  labels: ChartLabels,
  idPrefix: string,
): string {
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
    xlab += `<text x="${sc.x(t).toFixed(1)}" y="${H - 8}" text-anchor="${i === 0 ? "start" : i === COLS ? "end" : "middle"}" fill="var(--muted)" font-size="11">${escapeXml(labels.axisDate(t))}</text>`;
  }

  // Config periods: a band behind the curve per period, its name in the top
  // margin clipped a few px short of the band's edge so neighbours read
  // apart. The clip-path ids carry the caller's prefix because several charts
  // can share one document.
  let bands = "";
  periods.forEach((p, i) => {
    const x0 = sc.x(Math.max(p.start, sc.t0));
    const x1 = sc.x(Math.min(p.end, sc.t1));
    const w = x1 - x0;
    if (w < 1) return;
    const title = escapeXml(labels.periodTitle(p.label, fmtDate(p.start), fmtDate(p.end)));
    let label = "";
    if (w >= LABEL_MIN_PX) {
      const clip = `pb-${idPrefix}-${i}`;
      const text = `<text x="${(x0 + 4).toFixed(1)}" y="${M.t - 9}" clip-path="url(#${clip})" font-size="11" font-weight="600" fill="var(--switch)">${escapeXml(p.label)}</text>`;
      label =
        `<clipPath id="${clip}"><rect x="${x0.toFixed(1)}" y="0" width="${(w - 6).toFixed(1)}" height="${M.t}"/></clipPath>` +
        (p.href ? `<a data-href="${escapeXml(p.href)}">${text}</a>` : text);
    }
    bands += `<g class="period" data-i="${i}"><title>${title}</title><rect x="${x0.toFixed(1)}" y="${M.t}" width="${w.toFixed(1)}" height="${PH}"/>${label}</g>`;
  });

  // Config-switch markers: a dot sitting ON the return curve at each switch,
  // with a native hover tooltip naming the config it switched to.
  let sw = "";
  for (const c of switches) {
    if (c.ts < sc.t0 || c.ts > sc.t1) continue;
    const x = sc.x(c.ts).toFixed(1);
    const y = sc.y(returnAt(c.ts, pts)).toFixed(1);
    const title = escapeXml(labels.switchTitle(c.template_name, fmtDate(c.ts)));
    sw += `<circle cx="${x}" cy="${y}" r="5" fill="var(--switch)" stroke="var(--panel)" stroke-width="2"><title>${title}</title></circle>`;
  }

  const color = up ? "var(--pnl)" : "var(--pnl-neg)";
  const path = linePath(pts, sc);
  const bottom = (M.t + PH).toFixed(1);
  const area = `<path d="${path} L${sc.x(sc.t1).toFixed(1)} ${bottom} L${sc.x(sc.t0).toFixed(1)} ${bottom} Z" fill="${color}" opacity="0.08"/>`;
  const line = `<path d="${path}" fill="none" stroke="${color}" stroke-width="2" stroke-linejoin="round"/>`;

  return `<svg viewBox="0 0 ${W} ${H}" role="img" aria-label="${escapeXml(labels.ariaLabel)}">
    ${grid}${bands}${baseline}${xlab}${area}${sw}${line}
    <line data-part="cursor" x1="0" y1="${M.t}" x2="0" y2="${M.t + PH}" stroke="var(--accent)" stroke-width="1" opacity="0"/>
    <circle data-part="dot" r="3.5" fill="${color}" opacity="0"/>
    <rect data-part="hit" x="${M.l}" y="${M.t}" width="${PW}" height="${PH}" fill="transparent"/>
  </svg>`;
}

// Hover: a cursor line + dot snapped to the nearest daily point, the band of
// the config active on that day lit, and a fixed tooltip beside the pointer.
// Returns the teardown.
export function wireHover(
  container: HTMLElement,
  tip: HTMLElement,
  pts: ViewPoint[],
  labels: ChartLabels,
  periods: DrawnPeriod[] = [],
): () => void {
  const svg = container.querySelector("svg");
  const hit = container.querySelector<SVGElement>('[data-part="hit"]');
  const cursor = container.querySelector<SVGElement>('[data-part="cursor"]');
  const dot = container.querySelector<SVGElement>('[data-part="dot"]');
  if (!svg || !hit || !cursor || !dot) return () => {};
  const sc = scales(pts);
  const bands = Array.from(container.querySelectorAll<SVGElement>(".period"));

  // The period a day belongs to; at a switch instant the later one wins.
  const periodAt = (ts: number): number => {
    for (let i = periods.length - 1; i >= 0; i--) {
      const p = periods[i]!;
      if (ts >= p.start && ts <= p.end) return i;
    }
    return -1;
  };
  const light = (i: number) => {
    for (const b of bands) b.classList.toggle("on", b.getAttribute("data-i") === String(i));
  };

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
    const pi = periodAt(p.ts);
    light(pi);
    show(true);
    tip.innerHTML =
      `<div class="d">${fmtDate(p.ts)}</div>` +
      `<div class="row"><span>${escapeXml(labels.returnRow)}</span><b>${fmtPct(p.return_pct)}</b></div>` +
      (p.realized_usdt != null
        ? `<div class="row"><span>${escapeXml(labels.pnlRow)}</span><b>${fmtUsdt(p.realized_usdt)}</b></div>`
        : "") +
      (pi >= 0
        ? `<div class="row"><span>${escapeXml(labels.configRow)}</span><b>${escapeXml(periods[pi]!.label)}</b></div>`
        : "");
    tip.style.left = Math.min(e.clientX + 14, window.innerWidth - 150) + "px";
    tip.style.top = e.clientY + 14 + "px";
  };
  const leave = () => {
    light(-1);
    show(false);
  };

  hit.addEventListener("mousemove", move);
  hit.addEventListener("mouseleave", leave);
  return () => {
    hit.removeEventListener("mousemove", move);
    hit.removeEventListener("mouseleave", leave);
    leave();
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
  return String(s).replace(/[<>&"]/g, (c) => ({ "<": "&lt;", ">": "&gt;", "&": "&amp;", '"': "&quot;" })[c]!);
}
