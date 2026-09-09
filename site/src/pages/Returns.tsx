import { useState } from "react";
import { useLoad } from "../api/hooks";
import { RangeSelector, ReturnChart } from "../chart/ReturnChart";
import { DEFAULT_RANGE, fmtPct, selectWindow } from "../chart/returnCurve";
import { ErrorBanner, Loading } from "../components/ui";
import { staticData } from "../data/static";

// The public return-curve page: a bot selector over the published series, no
// token needed — the data is normalized and shows no account size.
export function Returns() {
  const index = useLoad(() => staticData.chartIndex(), "chart-index");
  const [chosen, setChosen] = useState<string | null>(null);
  const [range, setRange] = useState(DEFAULT_RANGE);
  const id = chosen ?? index.data?.[0]?.id ?? "";
  const series = useLoad(() => (id ? staticData.chart(id) : Promise.resolve(null)), `chart:${id}`);
  const win = series.data ? selectWindow(series.data, range) : null;
  const ok = win?.kind === "ok" ? win : null;

  return (
    <>
      <div className="page-head">
        <div>
          <h1>Bot Return Curves</h1>
          <div className="sub">Time-weighted return, collected daily from Bybit. Normalized — no account size shown.</div>
        </div>
      </div>
      <ErrorBanner error={index.error} onRetry={index.reload} />
      {index.data && index.data.length === 0 && <div className="msg">No bots have data yet.</div>}
      {index.data && index.data.length > 0 && (
        <>
          <div style={{ display: "flex", gap: 12, alignItems: "center", flexWrap: "wrap", margin: "0 0 16px" }}>
            <label htmlFor="bot" className="sub">
              Bot
            </label>
            <select id="bot" className="select" style={{ width: "auto", minWidth: 200 }} value={id} onChange={(e) => setChosen(e.target.value)}>
              {index.data.map((b) => (
                <option key={b.id} value={b.id}>
                  {b.name}
                </option>
              ))}
            </select>
            <div style={{ marginLeft: "auto" }}>
              <RangeSelector value={range} onChange={setRange} />
            </div>
          </div>
          {ok && (
            <div className="tiles" style={{ gridTemplateColumns: "repeat(auto-fit, minmax(150px, 1fr))" }}>
              <div className="tile">
                <div className="k">{ok.stats.label} return</div>
                <div className="v">{fmtPct(ok.stats.ret)}</div>
              </div>
              <div className="tile">
                <div className="k">Peak</div>
                <div className="v">{fmtPct(ok.stats.peak)}</div>
              </div>
              <div className="tile">
                <div className="k">Days</div>
                <div className="v">{ok.stats.days}</div>
              </div>
            </div>
          )}
          <div className="card tight">
            <ErrorBanner error={series.error} onRetry={series.reload} />
            {series.loading && !series.data && <Loading what={id} />}
            {win && <ReturnChart window={win} />}
            <div className="legend">
              <span>
                <i className="swatch" style={{ background: "var(--pnl)" }} /> Cumulative return
              </span>
              <span>
                <i className="swatch dot" /> Config switch
              </span>
            </div>
          </div>
          {ok?.footer && <div className="note" style={{ fontSize: 12 }}>{ok.footer}</div>}
        </>
      )}
    </>
  );
}
