import { useMemo, useRef, useState, type MouseEvent, type PointerEvent } from "react";
import { useLang, useT } from "../i18n/locale";
import {
  type ComboSeries,
  type Curve,
  DAY,
  PRESETS,
  type Span,
  clampSpan,
  presetOf,
  presetSpan,
  rebase,
  valueAt,
} from "./combo";
import { fmtDate, fmtSignedPct } from "./returnCurve";

// The chart and the brush share the horizontal margins so a date sits at the
// same x in both.
const W = 900,
  H = 300,
  M = { l: 56, r: 12, t: 14, b: 30 };
const PW = W - M.l - M.r,
  PH = H - M.t - M.b;
const BH = 56; // the brush strip's height
// Within this many viewBox units of a handle a press takes the handle.
const GRIP = 10;

function yScale(curves: Curve[]) {
  let vmin = 0,
    vmax = 0; // the 0% baseline is always in view
  for (const c of curves) {
    for (const p of c.view) {
      if (p.pct < vmin) vmin = p.pct;
      if (p.pct > vmax) vmax = p.pct;
    }
  }
  if (vmin === vmax) {
    vmin -= 1;
    vmax += 1;
  }
  const pad = (vmax - vmin) * 0.08;
  vmin -= pad;
  vmax += pad;
  return { vmin, vmax, y: (v: number) => M.t + (1 - (v - vmin) / (vmax - vmin)) * PH };
}

const xOf = (span: Span) => (t: number) => M.l + ((t - span.from) / (span.to - span.from || 1)) * PW;
const tOf = (span: Span) => (x: number) => span.from + ((x - M.l) / PW) * (span.to - span.from);

// The pointer's x in viewBox units.
function viewX(e: { clientX: number; currentTarget: Element }): number {
  const rect = e.currentTarget.getBoundingClientRect();
  return ((e.clientX - rect.left) / rect.width) * W;
}

function path(view: Curve["view"], x: (t: number) => number, y: (v: number) => number): string {
  return view.map((p, i) => `${i ? "L" : "M"}${x(p.ts).toFixed(1)} ${y(p.pct).toFixed(1)}`).join(" ");
}

// The preset buttons and the dates of the period in force; no button is lit
// while the brush holds a span of its own.
export function PresetBar({ domain, span, onSpan }: { domain: Span; span: Span; onSpan: (s: Span) => void }) {
  const on = presetOf(domain, span);
  return (
    <div className="presets">
      <div className="ranges" role="tablist">
        {PRESETS.map((p, i) => (
          <button
            key={p.k}
            type="button"
            role="tab"
            aria-selected={i === on}
            className={`range${i === on ? " on" : ""}`}
            onClick={() => onSpan(presetSpan(domain, p.days))}
          >
            {p.k}
          </button>
        ))}
      </div>
      <span className="span">
        {fmtDate(span.from)} → {fmtDate(span.to)}
      </span>
    </div>
  );
}

// The series over the chosen span, each re-based there, with a hover that
// names every curve present on the pointed day.
export function ComboChart({ series, span }: { series: ComboSeries[]; span: Span }) {
  const t = useT();
  const { lang } = useLang();
  const [hover, setHover] = useState<{ ts: number; cx: number; cy: number } | null>(null);
  const curves = useMemo(() => rebase(series, span), [series, span]);
  const { vmin, vmax, y } = useMemo(() => yScale(curves), [curves]);
  const x = xOf(span);
  const toTs = tOf(span);
  const month = useMemo(
    () => new Intl.DateTimeFormat(lang === "zh" ? "zh-CN" : "en-US", { month: "short", timeZone: "UTC" }),
    [lang],
  );
  // A period over a year long is ticked by month; a shorter one by day.
  const long = span.to - span.from > 366 * DAY;
  const tick = (sec: number) => {
    const d = new Date(sec * 1000);
    return long ? d.toISOString().slice(0, 7) : t.returns.chart.axisDate(month.format(d), d.getUTCDate());
  };

  if (curves.length === 0) return <div className="msg">{t.configs.chart.notEnough}</div>;

  const ROWS = 5,
    COLS = 5;
  const grid = Array.from({ length: ROWS + 1 }, (_, i) => vmin + ((vmax - vmin) * i) / ROWS);
  const cols = Array.from({ length: COLS + 1 }, (_, i) => span.from + ((span.to - span.from) * i) / COLS);
  const rows = hover
    ? curves
        .map((c) => ({ c, v: valueAt(c.view, hover.ts) }))
        .filter((r): r is { c: Curve; v: number } => r.v != null)
    : [];

  const move = (e: MouseEvent<SVGRectElement>) => {
    const vx = Math.min(M.l + PW, Math.max(M.l, viewX(e)));
    setHover({ ts: Math.min(span.to, Math.max(span.from, toTs(vx))), cx: e.clientX, cy: e.clientY });
  };

  return (
    <>
      <div className="chart">
        <svg viewBox={`0 0 ${W} ${H}`} role="img" aria-label={t.configs.chart.aria}>
          {grid.map((v) => (
            <g key={v}>
              <line x1={M.l} y1={y(v)} x2={W - M.r} y2={y(v)} stroke="var(--grid)" />
              <text x={M.l - 8} y={y(v)} textAnchor="end" dominantBaseline="middle" fill="var(--muted)" fontSize={11}>
                {v > 0 ? "+" : ""}
                {v.toFixed(1)}%
              </text>
            </g>
          ))}
          <line x1={M.l} y1={y(0)} x2={W - M.r} y2={y(0)} stroke="var(--muted)" strokeWidth={1} opacity={0.5} />
          {cols.map((c, i) => (
            <text
              key={c}
              x={x(c)}
              y={H - 8}
              textAnchor={i === 0 ? "start" : i === COLS ? "end" : "middle"}
              fill="var(--muted)"
              fontSize={11}
            >
              {tick(c)}
            </text>
          ))}
          {curves.map(({ series: s, view }) => (
            <path
              key={s.id}
              d={path(view, x, y)}
              fill="none"
              stroke={s.color}
              strokeWidth={s.dashed ? 1.5 : 2}
              strokeDasharray={s.dashed ? "5 4" : undefined}
              strokeLinejoin="round"
            />
          ))}
          {hover && (
            <>
              <line x1={x(hover.ts)} y1={M.t} x2={x(hover.ts)} y2={M.t + PH} stroke="var(--accent)" strokeWidth={1} />
              {rows.map(({ c, v }) => (
                <circle key={c.series.id} cx={x(hover.ts)} cy={y(v)} r={3.5} fill={c.series.color} />
              ))}
            </>
          )}
          <rect
            x={M.l}
            y={M.t}
            width={PW}
            height={PH}
            fill="transparent"
            onMouseMove={move}
            onMouseLeave={() => setHover(null)}
          />
        </svg>
      </div>
      <div
        className="tip"
        style={
          hover
            ? { opacity: 1, left: Math.min(hover.cx + 14, window.innerWidth - 200), top: hover.cy + 14 }
            : { opacity: 0 }
        }
      >
        {hover && (
          <>
            <div className="d">{fmtDate(hover.ts)}</div>
            {rows.map(({ c, v }) => (
              <div key={c.series.id} className="row">
                <span>
                  <i className="swatch" style={{ background: c.series.color }} /> {c.series.label}
                </span>
                <b>{fmtSignedPct(v)}</b>
              </div>
            ))}
          </>
        )}
      </div>
    </>
  );
}

type Drag =
  | { mode: "left" | "right" | "new"; anchor: number }
  | { mode: "move"; anchor: number; start: Span };

// The strip under the chart: the backtest over the whole domain, with the
// chosen span as a window the reader drags by its handles, moves as a whole,
// or draws afresh by pressing outside it.
export function Brush({
  overview,
  domain,
  span,
  onSpan,
}: {
  overview: ComboSeries;
  domain: Span;
  span: Span;
  onSpan: (s: Span) => void;
}) {
  const t = useT();
  const drag = useRef<Drag | null>(null);
  const x = xOf(domain);
  const toTs = tOf(domain);
  const [curve] = rebase([overview], domain);
  const line = useMemo(() => {
    if (!curve) return "";
    let lo = Infinity,
      hi = -Infinity;
    for (const p of curve.view) {
      if (p.pct < lo) lo = p.pct;
      if (p.pct > hi) hi = p.pct;
    }
    if (lo === hi) hi = lo + 1;
    const y = (v: number) => 4 + (1 - (v - lo) / (hi - lo)) * (BH - 8);
    return path(curve.view, x, y);
    // The domain is the only input that moves the strip's own curve.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [overview, domain.from, domain.to]);

  const x0 = x(span.from),
    x1 = x(span.to);

  const down = (e: PointerEvent<SVGSVGElement>) => {
    const vx = viewX(e);
    const ts = toTs(vx);
    if (Math.abs(vx - x0) <= GRIP) drag.current = { mode: "left", anchor: span.to };
    else if (Math.abs(vx - x1) <= GRIP) drag.current = { mode: "right", anchor: span.from };
    else if (vx > x0 && vx < x1) drag.current = { mode: "move", anchor: ts, start: span };
    else drag.current = { mode: "new", anchor: ts };
    e.currentTarget.setPointerCapture(e.pointerId);
  };
  const moved = (e: PointerEvent<SVGSVGElement>) => {
    const d = drag.current;
    if (!d) return;
    const ts = toTs(viewX(e));
    if (d.mode === "move") {
      const len = d.start.to - d.start.from;
      const from = Math.min(Math.max(d.start.from + ts - d.anchor, domain.from), domain.to - len);
      onSpan({ from, to: from + len });
    } else if (d.mode === "left") onSpan(clampSpan({ from: ts, to: d.anchor }, domain));
    else if (d.mode === "right") onSpan(clampSpan({ from: d.anchor, to: ts }, domain));
    else onSpan(clampSpan({ from: d.anchor, to: ts }, domain));
  };
  const up = () => {
    drag.current = null;
  };

  return (
    <svg
      className="brush"
      viewBox={`0 0 ${W} ${BH}`}
      role="slider"
      aria-label={t.configs.chart.brushAria}
      aria-valuetext={`${fmtDate(span.from)} → ${fmtDate(span.to)}`}
      onPointerDown={down}
      onPointerMove={moved}
      onPointerUp={up}
      onPointerCancel={up}
    >
      <rect x={M.l} y={0} width={PW} height={BH} fill="var(--panel)" />
      {line && <path d={line} fill="none" stroke="var(--balance)" strokeWidth={1.5} />}
      <rect className="window" x={x0} y={0} width={Math.max(0, x1 - x0)} height={BH} />
      <rect className="outline" x={x0} y={0.5} width={Math.max(0, x1 - x0)} height={BH - 1} />
      <rect className="handle" x={x0 - 3} y={BH / 2 - 10} width={6} height={20} rx={2} />
      <rect className="handle" x={x1 - 3} y={BH / 2 - 10} width={6} height={20} rx={2} />
    </svg>
  );
}
