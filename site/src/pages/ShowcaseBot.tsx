import { useState } from "react";
import { Link, useParams } from "react-router-dom";
import { useLoad } from "../api/hooks";
import { ChartCaption, RangeSelector, ReturnChart, useRangeLabel } from "../chart/ReturnChart";
import { DEFAULT_RANGE, fmtDate, fmtPct, fmtSignedPct, selectWindow } from "../chart/returnCurve";
import { asSeries } from "../chart/showcase";
import { Badge, Crumbs, ErrorBanner, Loading } from "../components/ui";
import { staticData } from "../data/static";
import { useT } from "../i18n/locale";

// One showcase bot's curve, from its public file: the owner's chart with the
// config periods, and no money tiles because the public series carries none.
export function ShowcaseBot() {
  const { pid = "" } = useParams();
  const t = useT();
  const rangeLabel = useRangeLabel();
  const [range, setRange] = useState(DEFAULT_RANGE);
  const { data, error, loading, reload } = useLoad(() => staticData.showcaseBot(pid), `showcase:${pid}`);
  const win = data ? selectWindow(asSeries(data), range) : null;
  const ok = win?.kind === "ok" ? win : null;

  return (
    <>
      <Crumbs items={[{ to: "/p", label: t.showcase.title }, { label: data?.name ?? <span className="mono">{pid}</span> }]} />
      <ErrorBanner error={error} onRetry={reload} />
      {loading && !data && <Loading what={t.showcase.loadingBot} />}
      {!loading && !error && !data && (
        <div className="msg">
          {t.showcase.notFound} <Link to="/p">{t.showcase.title}</Link>
        </div>
      )}
      {data && (
        <>
          <div className="page-head top">
            <div style={{ minWidth: 0 }}>
              <div style={{ display: "flex", alignItems: "center", gap: 10, flexWrap: "wrap" }}>
                <h1>{data.name}</h1>
                <Badge>{data.exchange.toUpperCase()}</Badge>
              </div>
              <div className="sub" style={{ marginTop: 4 }}>
                {t.showcase.currentReturn}: {fmtSignedPct(data.current_return_pct)} ·{" "}
                {t.showcase.updated(fmtDate(data.generated_at))}
              </div>
            </div>
            <a className="btn" href={data.public_url} target="_blank" rel="noopener noreferrer">
              {t.showcase.onBybit} ↗
            </a>
          </div>
          <div style={{ display: "flex", justifyContent: "flex-end", margin: "0 0 16px" }}>
            <RangeSelector value={range} onChange={setRange} />
          </div>
          {ok && (
            <div className="tiles">
              <div className="tile">
                <div className="k">{t.returns.tile.return(rangeLabel(ok.stats.label))}</div>
                <div className="v">{fmtPct(ok.stats.ret)}</div>
              </div>
              <div className="tile">
                <div className="k">{t.returns.tile.peak}</div>
                <div className="v">{fmtPct(ok.stats.peak)}</div>
              </div>
              <div className="tile">
                <div className="k">{t.bots.detail.maxDrawdownTile(rangeLabel(ok.stats.label))}</div>
                <div className="v">{fmtPct(ok.stats.maxDrawdown)}</div>
              </div>
              <div className="tile">
                <div className="k">{t.returns.tile.days}</div>
                <div className="v">{ok.stats.days}</div>
              </div>
            </div>
          )}
          <div className="card tight">
            {win && <ReturnChart window={win} />}
            <div className="legend">
              <span>
                <i className="swatch" style={{ background: "var(--pnl)" }} /> {t.returns.legend.cumulative}
              </span>
              <span>
                <i className="swatch dot" /> {t.returns.legend.configSwitch}
              </span>
            </div>
            <div className="hint" style={{ marginTop: 6 }}>
              {t.bots.detail.switchDot}
            </div>
          </div>
          <div className="note" style={{ fontSize: 12 }}>
            {ok?.caption && (
              <>
                <ChartCaption caption={ok.caption} /> ·{" "}
              </>
            )}
            {t.showcase.disclaimer}
          </div>
        </>
      )}
    </>
  );
}
