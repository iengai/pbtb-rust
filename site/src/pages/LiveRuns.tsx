import { useMemo } from "react";
import { Link } from "react-router-dom";
import { useLoad } from "../api/hooks";
import { ReturnChart } from "../chart/ReturnChart";
import { fmtDate, fmtSignedPct } from "../chart/returnCurve";
import { MAX_RUNS_PER_TEMPLATE, MIN_RUN_DAYS, fmtCap, runWindow, runsForTemplate } from "../chart/showcase";
import { staticData } from "../data/static";
import { useT } from "../i18n/locale";

// The live side of a config's page: each long enough span a showcase bot ran
// it, as its own re-based curve. Nothing to show renders nothing, so a config
// no public bot has run keeps the page it had.
export function LiveRuns({ template }: { template: string }) {
  const t = useT();
  const { data } = useLoad(() => staticData.showcaseBots(), "showcase:bots");
  const runs = useMemo(() => (data ? runsForTemplate(data, template) : []), [data, template]);
  if (runs.length === 0) return null;
  return (
    <div className="card" style={{ marginBottom: 18 }}>
      <div className="card-title">{t.showcase.runs.title}</div>
      <div className="hint" style={{ marginBottom: 4 }}>
        {t.showcase.runs.lead(MIN_RUN_DAYS, MAX_RUNS_PER_TEMPLATE)}
      </div>
      {runs.map((r) => {
        const up = r.return_pct >= 0;
        return (
          <div key={`${r.bot.id}:${r.start}`} className="run">
            <div className="head">
              <Link to={`/p/bots/${encodeURIComponent(r.bot.id)}`} style={{ fontWeight: 600, color: "var(--text)" }}>
                {r.bot.name}
              </Link>
              <a href={r.bot.public_url} target="_blank" rel="noopener noreferrer" style={{ fontSize: 13 }}>
                {t.showcase.onBybit} ↗
              </a>
              <span className="hint" style={{ marginLeft: "auto" }}>
                {t.showcase.runs.caption({
                  cap: fmtCap(r.cap_usdt),
                  start: fmtDate(r.start),
                  end: r.end != null ? fmtDate(r.end) : t.showcase.runs.ongoing,
                  days: r.days,
                  ret: fmtSignedPct(r.return_pct),
                })}
              </span>
            </div>
            <div style={{ color: up ? "var(--pnl)" : "var(--pnl-neg)" }}>
              <ReturnChart window={runWindow(r)} />
            </div>
          </div>
        );
      })}
    </div>
  );
}
