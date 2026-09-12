import { useEffect, useMemo, useRef, type MouseEvent } from "react";
import { useNavigate } from "react-router-dom";
import { useLoad } from "../api/hooks";
import { templateTitle } from "../components/ui";
import { staticData } from "../data/static";
import { useLang, useT } from "../i18n/locale";
import {
  type ChartLabels,
  type ChartWindow,
  type DrawnPeriod,
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
      configRow: t.returns.chart.configRow,
      switchTitle: t.returns.chart.switchTitle,
      periodTitle: t.returns.chart.periodTitle,
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
  const navigate = useNavigate();
  const box = useRef<HTMLDivElement>(null);
  const tip = useRef<HTMLDivElement>(null);
  const { lang } = useLang();
  // A switch row quotes the template's id; the marker and the band name it by
  // its title, and an id the catalogue no longer lists (a retired template)
  // as itself, with no page to link to.
  const catalog = useLoad(() => staticData.templates(), "templates:titles");
  const switches = useMemo(() => {
    if (win.kind !== "ok") return [];
    return win.switches.map((s) => {
      const tpl = catalog.data?.find((row) => row.name === s.template_name);
      return tpl ? { ...s, template_name: templateTitle(tpl, lang) } : s;
    });
  }, [win, catalog.data, lang]);
  const periods = useMemo<DrawnPeriod[]>(() => {
    if (win.kind !== "ok") return [];
    return win.periods.map((p) => {
      const tpl = catalog.data?.find((row) => row.name === p.template_name);
      return {
        ...p,
        label: tpl ? templateTitle(tpl, lang) : p.template_name,
        href: tpl ? `/configs/${encodeURIComponent(tpl.name)}` : null,
      };
    });
  }, [win, catalog.data, lang]);
  const svg = win.kind === "ok" ? chartSVG(win.view, switches, periods, labels) : "";

  useEffect(() => {
    if (win.kind !== "ok" || !box.current || !tip.current) return;
    return wireHover(box.current, tip.current, win.view, labels, periods);
  }, [win, svg, labels, periods]);

  // The band labels are markup, not router links; a click on one is routed
  // here so the page changes without a reload.
  const onClick = (e: MouseEvent<HTMLDivElement>) => {
    const href = (e.target as Element).closest("a[data-href]")?.getAttribute("data-href");
    if (!href) return;
    e.preventDefault();
    navigate(href);
  };

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
      <div ref={box} className="chart" onClick={onClick} dangerouslySetInnerHTML={{ __html: svg }} />
      <div ref={tip} className="tip" />
    </>
  );
}
