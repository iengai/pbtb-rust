import { useEffect, useLayoutEffect, useMemo, useRef, useState, type PointerEvent } from "react";
import { useLang, useT } from "../i18n/locale";
import {
  type ComboSeries,
  type Curve,
  DAY,
  PRESETS,
  type Scale,
  type Span,
  axisOf,
  clampSpan,
  fmtTick,
  presetOf,
  presetSpan,
  rebase,
  valueAt,
} from "./combo";
import { type Frame, type Margins, frameOf } from "./frame";
import { fmtDate, fmtSignedPct, placeTip } from "./returnCurve";
import { useWidth } from "./useWidth";

// The chart and the brush share the horizontal margins so a date sits at the
// same x in both.
const M: Margins = { l: 56, r: 12, t: 14, b: 30 };
const BH = 56; // the brush strip's height
// Within this many pixels of a handle a press takes the handle; a fingertip
// is far less precise than a mouse pointer.
const GRIP = 10;
const TOUCH_GRIP = 22;

const xOf = (f: Frame, span: Span) => (t: number) => f.M.l + ((t - span.from) / (span.to - span.from || 1)) * f.PW;
const tOf = (f: Frame, span: Span) => (x: number) => span.from + ((x - f.M.l) / f.PW) * (span.to - span.from);

// The pointer's x in viewBox units.
function viewX(e: { clientX: number; currentTarget: Element }, W: number): number {
  const rect = e.currentTarget.getBoundingClientRect();
  return ((e.clientX - rect.left) / rect.width) * W;
}

function path(view: Curve["view"], x: (t: number) => number, y: (v: number) => number): string {
  return view.map((p, i) => `${i ? "L" : "M"}${x(p.ts).toFixed(1)} ${y(p.pct).toFixed(1)}`).join(" ");
}

// The preset buttons, the axis scale toggle and the dates of the period in
// force; no preset is lit while the brush holds a span of its own.
export function PresetBar({
  domain,
  span,
  onSpan,
  scale,
  onScale,
}: {
  domain: Span;
  span: Span;
  onSpan: (s: Span) => void;
  scale: Scale;
  onScale: (s: Scale) => void;
}) {
  const t = useT();
  const on = presetOf(domain, span);
  return (
    <div className="presets">
      <div style={{ display: "flex", gap: 8, flexWrap: "wrap" }}>
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
        <div className="ranges" role="tablist">
          {(["log", "linear"] as const).map((s) => (
            <button
              key={s}
              type="button"
              role="tab"
              aria-selected={scale === s}
              className={`range${scale === s ? " on" : ""}`}
              onClick={() => onScale(s)}
            >
              {t.configs.chart.scale[s]}
            </button>
          ))}
        </div>
      </div>
      <span className="span">
        {fmtDate(span.from)} → {fmtDate(span.to)}
      </span>
    </div>
  );
}

// The series over the chosen span, each re-based there and drawn on the
// chosen scale, with a hover that names every curve present on the pointed
// day. A touch reads the same way: a tap or a sideways drag moves the cursor,
// and the reading stays after the finger lifts until a press elsewhere.
export function ComboChart({ series, span, scale }: { series: ComboSeries[]; span: Span; scale: Scale }) {
  const t = useT();
  const { lang } = useLang();
  const [measure, width, narrow] = useWidth();
  const f = frameOf(width, M, narrow);
  const { W, H, PW, PH } = f;
  const svgRef = useRef<SVGSVGElement>(null);
  const tipRef = useRef<HTMLDivElement>(null);
  const [hover, setHover] = useState<{ ts: number; cx: number; cy: number; touch: boolean } | null>(null);
  const curves = useMemo(() => rebase(series, span), [series, span]);
  const axis = useMemo(() => axisOf(curves, scale), [curves, scale]);
  const y = (v: number) => M.t + (1 - axis.pos(v)) * PH;
  const x = xOf(f, span);
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

  // The tooltip is fixed to the viewport, so a scroll hides the reading rather
  // than leaving it behind; a touch reading also ends at a press outside the chart.
  const shown = hover != null;
  const touching = hover?.touch ?? false;
  useEffect(() => {
    if (!shown) return;
    const hide = () => setHover(null);
    const outside = (e: globalThis.PointerEvent) => {
      if (!svgRef.current?.contains(e.target as Node)) hide();
    };
    window.addEventListener("scroll", hide, { capture: true, passive: true });
    if (touching) document.addEventListener("pointerdown", outside);
    return () => {
      window.removeEventListener("scroll", hide, { capture: true });
      document.removeEventListener("pointerdown", outside);
    };
  }, [shown, touching]);
  // The tooltip is placed from its own rendered size, so after every render.
  useLayoutEffect(() => {
    if (hover && tipRef.current) placeTip(tipRef.current, hover.cx, hover.cy, hover.touch);
  });

  if (curves.length === 0) return <div className="msg">{t.configs.chart.notEnough}</div>;

  const cols = Array.from({ length: f.cols + 1 }, (_, i) => span.from + ((span.to - span.from) * i) / f.cols);
  const rows = hover
    ? curves
        .map((c) => ({ c, v: valueAt(c.view, hover.ts) }))
        .filter((r): r is { c: Curve; v: number } => r.v != null)
    : [];

  // The handlers sit on the plot rect, so the pointer's fraction of the
  // rect's own box is its fraction of the span.
  const move = (e: PointerEvent<SVGRectElement>) => {
    const rect = e.currentTarget.getBoundingClientRect();
    const frac = Math.min(1, Math.max(0, (e.clientX - rect.left) / rect.width));
    setHover({
      ts: span.from + frac * (span.to - span.from),
      cx: e.clientX,
      cy: e.clientY,
      touch: e.pointerType !== "mouse",
    });
  };

  return (
    <>
      <div className="chart" ref={measure}>
        <svg ref={svgRef} viewBox={`0 0 ${W} ${H}`} role="img" aria-label={t.configs.chart.aria}>
          {axis.ticks.map((v) => (
            <g key={v}>
              <line x1={M.l} y1={y(v)} x2={W - M.r} y2={y(v)} stroke="var(--grid)" />
              <text x={M.l - 8} y={y(v)} textAnchor="end" dominantBaseline="middle" fill="var(--muted)" fontSize={11}>
                {fmtTick(v)}
              </text>
            </g>
          ))}
          <line x1={M.l} y1={y(0)} x2={W - M.r} y2={y(0)} stroke="var(--muted)" strokeWidth={1} opacity={0.5} />
          {cols.map((c, i) => (
            <text
              key={c}
              x={x(c)}
              y={H - 8}
              textAnchor={i === 0 ? "start" : i === f.cols ? "end" : "middle"}
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
            onPointerMove={move}
            onPointerDown={move}
            onPointerLeave={(e) => e.pointerType === "mouse" && setHover(null)}
            onPointerCancel={() => setHover(null)}
          />
        </svg>
      </div>
      <div ref={tipRef} className="tip" style={{ opacity: hover ? 1 : 0 }}>
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
  const [measure, width, narrow] = useWidth();
  const f = frameOf(width, M, narrow);
  const drag = useRef<Drag | null>(null);
  const x = xOf(f, domain);
  const toTs = tOf(f, domain);
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
    // The domain and the drawn width are the only inputs that move the strip's own curve.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [overview, domain.from, domain.to, f.W]);

  const x0 = x(span.from),
    x1 = x(span.to);

  const down = (e: PointerEvent<SVGSVGElement>) => {
    const vx = viewX(e, f.W);
    const ts = toTs(vx);
    const grip = e.pointerType === "mouse" ? GRIP : TOUCH_GRIP;
    if (Math.abs(vx - x0) <= grip) drag.current = { mode: "left", anchor: span.to };
    else if (Math.abs(vx - x1) <= grip) drag.current = { mode: "right", anchor: span.from };
    else if (vx > x0 && vx < x1) drag.current = { mode: "move", anchor: ts, start: span };
    else drag.current = { mode: "new", anchor: ts };
    e.currentTarget.setPointerCapture(e.pointerId);
  };
  const moved = (e: PointerEvent<SVGSVGElement>) => {
    const d = drag.current;
    if (!d) return;
    const ts = toTs(viewX(e, f.W));
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
      ref={measure}
      className="brush"
      viewBox={`0 0 ${f.W} ${BH}`}
      role="slider"
      aria-label={t.configs.chart.brushAria}
      aria-valuetext={`${fmtDate(span.from)} → ${fmtDate(span.to)}`}
      onPointerDown={down}
      onPointerMove={moved}
      onPointerUp={up}
      onPointerCancel={up}
    >
      <rect x={M.l} y={0} width={f.PW} height={BH} fill="var(--panel)" />
      {line && <path d={line} fill="none" stroke="var(--balance)" strokeWidth={1.5} />}
      <rect className="window" x={x0} y={0} width={Math.max(0, x1 - x0)} height={BH} />
      <rect className="outline" x={x0} y={0.5} width={Math.max(0, x1 - x0)} height={BH - 1} />
      <rect className="handle" x={x0 - 3} y={BH / 2 - 10} width={6} height={20} rx={2} />
      <rect className="handle" x={x1 - 3} y={BH / 2 - 10} width={6} height={20} rx={2} />
    </svg>
  );
}
