import { useEffect, useMemo, useRef } from "react";
import { useLang, useT } from "../i18n/locale";
import {
  type ChartLabels,
  type ChartWindow,
  type RangeLabel,
  RANGES,
  type WindowCaption,
  chartSVG,
  fmtDate,
  wireHover,
} from "./returnCurve";

export function RangeSelector({ value, onChange }: { value: number; onChange: (i: number) => void }) {
  return (
    <div className="ranges" role="tablist">
      {RANGES.map((r, i) => (
        <button
          key={r.k}
          type="button"
          role="tab"
          aria-selected={i === value}
          className={`range${i === value ? " on" : ""}`}
          onClick={() => onChange(i)}
        >
          {r.k}
        </button>
      ))}
    </div>
  );
}

// The name of the window a curve covers: a re-funding era, all of history, or
// the preset's own key ("90D"), which reads the same in every language.
export function useRangeLabel(): (label: RangeLabel) => string {
  const t = useT();
  return (label) =>
    label.kind === "sinceRefunding"
      ? t.returns.range.sinceRefunding
      : label.kind === "total"
        ? t.returns.range.total
        : label.k;
}

// The provenance line under a chart: exchange, span, method, era and freshness.
export function ChartCaption({ caption }: { caption: WindowCaption }) {
  const t = useT();
  return (
    <>
      {t.returns.chart.caption({
        exchange: caption.exchange,
        days: caption.days,
        resetAt: caption.resetAt != null ? fmtDate(caption.resetAt) : null,
        updatedAt: fmtDate(caption.updatedAt),
      })}
    </>
  );
}

// The words the string-building SVG code needs, resolved for the current
// language. Axis ticks carry a month name, so those come from Intl.
function useChartLabels(): ChartLabels {
  const t = useT();
  const { lang } = useLang();
  return useMemo(() => {
    const month = new Intl.DateTimeFormat(lang === "zh" ? "zh-CN" : "en-US", {
      month: "short",
      timeZone: "UTC",
    });
    return {
      ariaLabel: t.returns.chart.aria,
      returnRow: t.returns.chart.returnRow,
      pnlRow: t.returns.chart.pnlRow,
      switchTitle: t.returns.chart.switchTitle,
      axisDate: (sec: number) => {
        const d = new Date(sec * 1000);
        return t.returns.chart.axisDate(month.format(d), d.getUTCDate());
      },
    };
  }, [lang, t]);
}

// The cumulative-return chart for one selected window. The SVG is produced as
// markup by `chartSVG` and the hover is wired imperatively, so the drawing code
// stays the dependency-free string builder the return-curve site was written as.
export function ReturnChart({ window: win }: { window: ChartWindow }) {
  const t = useT();
  const rangeLabel = useRangeLabel();
  const labels = useChartLabels();
  const box = useRef<HTMLDivElement>(null);
  const tip = useRef<HTMLDivElement>(null);
  const svg = win.kind === "ok" ? chartSVG(win.view, win.switches, labels) : "";

  useEffect(() => {
    if (win.kind !== "ok" || !box.current || !tip.current) return;
    return wireHover(box.current, tip.current, win.view, labels);
  }, [win, svg, labels]);

  if (win.kind === "empty") {
    const e = t.returns.chart.empty;
    const [message, hint] =
      win.reason === "noData"
        ? [e.noData, null]
        : win.reason === "resetAfterWindow"
          ? [e.refunded(fmtDate(win.resetAt)), e.refundedHint]
          : [e.wipedOut(rangeLabel(win.label)), e.wipedOutHint];
    return (
      <div className="msg">
        {message}
        {hint && (
          <>
            <br />
            <span className="hint">{hint}</span>
          </>
        )}
      </div>
    );
  }
  return (
    <>
      <div ref={box} className="chart" dangerouslySetInnerHTML={{ __html: svg }} />
      <div ref={tip} className="tip" />
    </>
  );
}
