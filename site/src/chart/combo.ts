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

// The colours the runs take, in the order the runs are listed (newest first).
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

// The runs drawn before the reader touches anything: the newest one, so the
// chart opens with a comparison and not a tangle.
export function initialSelection<T extends { key: string }>(runs: T[]): Set<string> {
  return new Set(runs.length ? [runs[0]!.key] : []);
}
