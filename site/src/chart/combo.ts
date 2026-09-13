// The config page's chart: a template's backtest and the live runs of that
// template on the showcase bots, drawn together over a period the reader
// chooses. Every curve arrives as an index (100 at its own start, or 1 + the
// run's return) and is re-based to 0% at its first point inside the period,
// so a backtest and a run over the same days read as returns over those days.
// Pure: the drawing and the pointer handling live in ComboChart.tsx.

export const DAY = 86400;

export type IndexPoint = { ts: number; v: number };

export type ComboSeries = {
  id: string;
  label: string;
  color: string;
  /** A supporting line (the backtest balance), drawn thin and dashed. */
  dashed?: boolean;
  points: IndexPoint[];
};

/** A closed period on the time axis, seconds. */
export type Span = { from: number; to: number };

/** One series inside a span: its points there as % from the first of them. */
export type Curve = { series: ComboSeries; view: { ts: number; pct: number }[] };

// Preset look-back periods, anchored at the latest point any curve reaches.
export const PRESETS: { k: string; days: number | null }[] = [
  { k: "1M", days: 30 },
  { k: "3M", days: 90 },
  { k: "6M", days: 180 },
  { k: "1Y", days: 365 },
  { k: "All", days: null },
];

// A span shorter than this has nothing to draw between its two ends.
export const MIN_SPAN_DAYS = 2;

// At or below this the account was wiped out; nothing can be re-based on it.
const DEAD_EPS = 1e-9;

// The colours the listed bots take, in list order (newest run first).
// Kept apart from the backtest's accent blue and the balance's grey, and
// legible on both themes.
export const RUN_COLORS = ["#d97706", "#16a34a", "#db2777", "#7c3aed", "#0891b2"];

// The earliest and latest point across the series; null when nothing has two
// points.
export function domainOf(series: ComboSeries[]): Span | null {
  let from = Infinity,
    to = -Infinity;
  for (const s of series) {
    for (const p of s.points) {
      if (p.ts < from) from = p.ts;
      if (p.ts > to) to = p.ts;
    }
  }
  return from < to ? { from, to } : null;
}

// The span a preset names inside a domain: the last `days` before the
// domain's end, or the whole domain.
export function presetSpan(domain: Span, days: number | null): Span {
  if (days == null) return { ...domain };
  return { from: Math.max(domain.from, domain.to - days * DAY), to: domain.to };
}

// The preset whose span this is, or -1 for a span the brush chose. The last
// match wins: on a domain shorter than a preset that preset's span is the
// whole domain, and "All" is the truthful name for it.
export function presetOf(domain: Span, span: Span): number {
  for (let i = PRESETS.length - 1; i >= 0; i--) {
    const s = presetSpan(domain, PRESETS[i]!.days);
    if (s.from === span.from && s.to === span.to) return i;
  }
  return -1;
}

// A span the brush proposes, made drawable: inside the domain, in order, and
// at least MIN_SPAN_DAYS long (grown to the left, or to the right at the
// domain's start).
export function clampSpan(span: Span, domain: Span): Span {
  const clamp = (t: number) => Math.min(domain.to, Math.max(domain.from, t));
  let from = clamp(Math.min(span.from, span.to));
  let to = clamp(Math.max(span.from, span.to));
  const min = Math.min(MIN_SPAN_DAYS * DAY, domain.to - domain.from);
  if (to - from < min) {
    from = Math.max(domain.from, to - min);
    to = Math.min(domain.to, from + min);
  }
  return { from, to };
}

// Each series re-based to its first point inside the span. A series with
// fewer than two points there, or one whose first point is a wiped account,
// is left out rather than drawn as a dot or an invented percentage.
export function rebase(series: ComboSeries[], span: Span): Curve[] {
  const out: Curve[] = [];
  for (const s of series) {
    const inside = s.points.filter((p) => p.ts >= span.from && p.ts <= span.to);
    if (inside.length < 2) continue;
    const base = inside[0]!.v;
    if (!(base > DEAD_EPS)) continue;
    out.push({ series: s, view: inside.map((p) => ({ ts: p.ts, pct: (p.v / base - 1) * 100 })) });
  }
  return out;
}

export type Scale = "log" | "linear";

// The floor a wiped account sits at on the log axis (−99%), so a curve that
// reaches ×0 is clipped there instead of falling off the chart.
const LOG_FLOOR = 0.01;

// A curve value as a multiple of the period's first point.
const mult = (pct: number) => Math.max(LOG_FLOOR, 1 + pct / 100);

// The scale's position of a value, before padding: the multiple's log, or
// the % itself.
const raw = (scale: Scale) => (scale === "log" ? (pct: number) => Math.log(mult(pct)) : (pct: number) => pct);

export type Axis = {
  /** The grid rows, bottom first, in %. */
  ticks: number[];
  /** A value in % → its height in the plot, 0 at the bottom, 1 at the top. */
  pos: (pct: number) => number;
};

const ROWS = 5;

// The y-axis for the curves in view. The 0% baseline is always in view; the
// range is padded above, and below only where a curve goes under 0%, so a
// chart of curves that only gain starts at 0%. The rows are even steps of
// the scale: multiples of the first point on the log axis, % on the linear.
export function axisOf(curves: Curve[], scale: Scale): Axis {
  let vmin = 0,
    vmax = 0;
  for (const c of curves) {
    for (const p of c.view) {
      if (p.pct < vmin) vmin = p.pct;
      if (p.pct > vmax) vmax = p.pct;
    }
  }
  const at = raw(scale);
  let lo = at(vmin),
    hi = at(vmax);
  if (lo === hi) hi += at(1) - at(0); // a flat line sits on the baseline row
  const pad = (hi - lo) * 0.08;
  hi += pad;
  if (vmin < 0) lo -= pad;
  const back = scale === "log" ? (v: number) => (Math.exp(v) - 1) * 100 : (v: number) => v;
  return {
    ticks: Array.from({ length: ROWS + 1 }, (_, i) => back(lo + ((hi - lo) * i) / ROWS)),
    pos: (pct) => (at(pct) - lo) / (hi - lo),
  };
}

// A grid row's label: whole percent from a hundred up, one decimal below.
export function fmtTick(pct: number): string {
  const v = Math.abs(pct) >= 100 ? pct.toFixed(0) : pct.toFixed(1);
  return `${pct > 0 ? "+" : ""}${v}%`;
}

// The curve's % at a time inside its range, linearly interpolated; null
// outside it, so a tooltip names only the curves that exist on that day.
export function valueAt(view: Curve["view"], ts: number): number | null {
  const n = view.length;
  if (n === 0 || ts < view[0]!.ts || ts > view[n - 1]!.ts) return null;
  for (let i = 1; i < n; i++) {
    if (view[i]!.ts >= ts) {
      const a = view[i - 1]!,
        b = view[i]!;
      const f = (ts - a.ts) / (b.ts - a.ts || 1);
      return a.pct + f * (b.pct - a.pct);
    }
  }
  return view[n - 1]!.pct;
}

// What is drawn before the reader touches anything: the first listed entry
// (the bot with the newest run), so the chart opens with a comparison and not
// a tangle.
export function initialSelection<T extends { key: string }>(listed: T[]): Set<string> {
  return new Set(listed.length ? [listed[0]!.key] : []);
}
