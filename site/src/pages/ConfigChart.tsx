import { useMemo, useState } from "react";
import { Link } from "react-router-dom";
import { useLoad } from "../api/hooks";
import { Brush, ComboChart, PresetBar } from "../chart/ComboChart";
import { type ComboSeries, RUN_COLORS, type Span, domainOf, initialSelection, presetSpan } from "../chart/combo";
import { fmtDate, fmtSignedPct } from "../chart/returnCurve";
import { MAX_RUNS_PER_TEMPLATE, MIN_RUN_DAYS, type Run, fmtCap, runsForTemplate } from "../chart/showcase";
import { type TemplateBacktest, staticData } from "../data/static";
import { useT } from "../i18n/locale";

type ListedRun = { run: Run; key: string; color: string };

// A config's chart card: the backtest and the live runs of the config on the
// showcase bots on one chart, a period to choose, and the run list with a
// checkbox per run. The newest run is drawn first; a config no public bot
// has run shows the backtest alone and no list.
export function ConfigChart({ template }: { template: TemplateBacktest }) {
  const t = useT();
  const bots = useLoad(() => staticData.showcaseBots(), "showcase:bots");
  const runs = useMemo<ListedRun[]>(
    () =>
      (bots.data ? runsForTemplate(bots.data, template.name) : []).map((run, i) => ({
        run,
        key: `${run.bot.id}:${run.start}`,
        color: RUN_COLORS[i % RUN_COLORS.length]!,
      })),
    [bots.data, template.name],
  );
  // Untouched, the selection is the newest run; a click takes over from there.
  const [picked, setPicked] = useState<Set<string> | null>(null);
  const selected = picked ?? initialSelection(runs);

  const backtest = useMemo<ComboSeries[]>(
    () => [
      {
        id: "equity",
        label: t.configs.chart.backtest,
        color: "var(--accent)",
        points: template.points.map((p) => ({ ts: p.ts, v: p.equity })),
      },
      {
        id: "balance",
        label: t.configs.chart.balance,
        color: "var(--balance)",
        dashed: true,
        points: template.points.map((p) => ({ ts: p.ts, v: p.balance })),
      },
    ],
    [template.points, t],
  );
  // A run's points are indexed from the close before its switch; that base
  // is the series' first point, so a period opening before the run re-bases
  // it where the caption does.
  const live = useMemo<ComboSeries[]>(
    () =>
      runs.map((r) => ({
        id: r.key,
        label: r.run.bot.name,
        color: r.color,
        points: [{ ts: r.run.start, v: 1 }, ...r.run.points.map((p) => ({ ts: p.ts, v: 1 + p.return_pct / 100 }))],
      })),
    [runs],
  );
  // The domain takes every run, ticked or not, so toggling one never moves
  // the axis under the reader.
  const domain = useMemo(() => domainOf([...backtest, ...live]), [backtest, live]);
  const [chosen, setChosen] = useState<Span | null>(null);
  const span = chosen && domain ? chosen : domain ? presetSpan(domain, null) : null;
  const shown = useMemo(() => [...backtest, ...live.filter((s) => selected.has(s.id))], [backtest, live, selected]);

  const toggle = (key: string) =>
    setPicked((prev) => {
      const next = new Set(prev ?? selected);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });

  return (
    <div className="card tight" style={{ marginBottom: 18 }}>
      <div className="card-title" style={{ marginBottom: 4, padding: "0 4px" }}>
        {t.configs.chart.title}
      </div>
      <div className="hint" style={{ marginBottom: 8, padding: "0 4px" }}>
        {t.configs.chart.hint}
      </div>
      {domain && span ? (
        <>
          <PresetBar domain={domain} span={span} onSpan={setChosen} />
          <ComboChart series={shown} span={span} />
          <Brush overview={backtest[0]!} domain={domain} span={span} onSpan={setChosen} />
        </>
      ) : (
        <div className="msg">{t.configs.chart.notEnough}</div>
      )}
      <div className="legend">
        <span>
          <i className="swatch" style={{ background: "var(--accent)" }} /> {t.configs.chart.backtest}
        </span>
        <span>
          <i className="swatch dash" /> {t.configs.chart.balance}
        </span>
      </div>
      {runs.length > 0 && (
        <div style={{ marginTop: 14, padding: "0 4px" }}>
          <div className="card-title sm">{t.showcase.runs.title}</div>
          <div className="hint" style={{ marginBottom: 6 }}>
            {t.showcase.runs.lead(MIN_RUN_DAYS, MAX_RUNS_PER_TEMPLATE)}
          </div>
          {runs.map(({ run: r, key, color }) => (
            <div key={key} className="run-row">
              <label>
                <input type="checkbox" checked={selected.has(key)} onChange={() => toggle(key)} />
                <i className="swatch" style={{ background: color }} />
              </label>
              <Link to={`/p/bots/${encodeURIComponent(r.bot.id)}`} style={{ fontWeight: 600, color: "var(--text)" }}>
                {r.bot.name}
              </Link>
              <a href={r.bot.public_url} target="_blank" rel="noopener noreferrer" style={{ fontSize: 13 }}>
                {t.showcase.onBybit} ↗
              </a>
              <span className="hint cap" style={{ color: r.return_pct >= 0 ? "var(--pnl)" : "var(--pnl-neg)" }}>
                {t.showcase.runs.caption({
                  cap: fmtCap(r.cap_usdt),
                  start: fmtDate(r.start),
                  end: r.end != null ? fmtDate(r.end) : t.showcase.runs.ongoing,
                  days: r.days,
                  ret: fmtSignedPct(r.return_pct),
                })}
              </span>
            </div>
          ))}
          <div className="note" style={{ fontSize: 12, marginTop: 8 }}>
            {t.showcase.disclaimer}
          </div>
        </div>
      )}
    </div>
  );
}
