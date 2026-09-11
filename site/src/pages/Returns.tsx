import { useState } from "react";
import { api } from "../api/client";
import { useLoad } from "../api/hooks";
import { ChartCaption, RangeSelector, ReturnChart, useRangeLabel } from "../chart/ReturnChart";
import { DEFAULT_RANGE, fmtPct, fmtUsdt, selectWindow } from "../chart/returnCurve";
import { ErrorBanner, Loading } from "../components/ui";
import { useT } from "../i18n/locale";
import { loadReturns } from "./returnsApi";

// The return-curve page: a selector over the signed-in account's own bots. The
// series carries a return index and the realized PnL in USDT, never a balance,
// and it is the owner's alone; the API reads it under their tenant.
export function Returns() {
  const t = useT();
  const rangeLabel = useRangeLabel();
  const bots = useLoad(() => api.listBots(), "bots");
  const [chosen, setChosen] = useState<string | null>(null);
  const [range, setRange] = useState(DEFAULT_RANGE);
  const id = chosen ?? bots.data?.bots[0]?.bot_id ?? "";
  const series = useLoad(() => (id ? loadReturns(id) : Promise.resolve(null)), `returns:${id}`);
  const win = series.data ? selectWindow(series.data, range) : null;
  const ok = win?.kind === "ok" ? win : null;

  return (
    <>
      <div className="page-head">
        <div>
          <h1>{t.returns.title}</h1>
          <div className="sub">{t.returns.lead}</div>
        </div>
      </div>
      <ErrorBanner error={bots.error} onRetry={bots.reload} />
      {bots.loading && !bots.data && <Loading what={t.returns.loadingBots} />}
      {bots.data && bots.data.bots.length === 0 && <div className="msg">{t.returns.noBots}</div>}
      {bots.data && bots.data.bots.length > 0 && (
        <>
          <div style={{ display: "flex", gap: 12, alignItems: "center", flexWrap: "wrap", margin: "0 0 16px" }}>
            <label htmlFor="bot" className="sub">
              {t.returns.botLabel}
            </label>
            <select id="bot" className="select" style={{ width: "auto", minWidth: 200 }} value={id} onChange={(e) => setChosen(e.target.value)}>
              {bots.data.bots.map((b) => (
                <option key={b.bot_id} value={b.bot_id}>
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
                <div className="k">{t.returns.tile.return(rangeLabel(ok.stats.label))}</div>
                <div className="v">{fmtPct(ok.stats.ret)}</div>
              </div>
              <div className="tile">
                <div className="k">{t.returns.tile.peak}</div>
                <div className="v">{fmtPct(ok.stats.peak)}</div>
              </div>
              <div className="tile">
                <div className="k">{t.returns.tile.days}</div>
                <div className="v">{ok.stats.days}</div>
              </div>
              <div className="tile">
                <div className="k">{t.returns.tile.pnl(rangeLabel(ok.stats.label))}</div>
                <div className="v">{fmtUsdt(ok.stats.pnl)}</div>
              </div>
              <div className="tile">
                <div className="k">{t.returns.tile.totalPnl}</div>
                <div className="v">{fmtUsdt(ok.stats.totalPnl)}</div>
              </div>
            </div>
          )}
          <div className="card tight">
            <ErrorBanner error={series.error} onRetry={series.reload} />
            {series.loading && !series.data && <Loading what={t.returns.loadingCurve} />}
            {series.data === null && !series.loading && <div className="msg">{t.returns.noData}</div>}
            {win && <ReturnChart window={win} />}
            <div className="legend">
              <span>
                <i className="swatch" style={{ background: "var(--pnl)" }} /> {t.returns.legend.cumulative}
              </span>
              <span>
                <i className="swatch dot" /> {t.returns.legend.configSwitch}
              </span>
            </div>
          </div>
          {ok?.caption && (
            <div className="note" style={{ fontSize: 12 }}>
              <ChartCaption caption={ok.caption} />
            </div>
          )}
        </>
      )}
    </>
  );
}
