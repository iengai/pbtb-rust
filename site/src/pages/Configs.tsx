import { useMemo, useState } from "react";
import { Link } from "react-router-dom";
import { useLoad } from "../api/hooks";
import { Badge, Chips, ErrorBanner, Loading, Sparkline, engineLabel } from "../components/ui";
import { staticData, type TemplateSummary } from "../data/static";
import { fmtGain, fmtMetric, wipedOut } from "./metrics";

export function Configs() {
  const { data, error, loading, reload } = useLoad(() => staticData.templates(), "templates");
  const [engine, setEngine] = useState<string>("all");

  const engines = useMemo(
    () => Array.from(new Set((data ?? []).map((t) => t.engine))).sort().reverse(),
    [data],
  );
  const shown = useMemo(() => {
    const list = (data ?? []).filter((t) => engine === "all" || t.engine === engine);
    return list.sort((a, b) => (b.metrics.gain ?? 0) - (a.metrics.gain ?? 0));
  }, [data, engine]);
  const exchanges = Array.from(new Set(shown.map((t) => t.exchange))).join(", ");

  return (
    <>
      <div className="page-head">
        <div>
          <h1>Configs</h1>
          <div className="sub">
            {data
              ? `${shown.length} of ${data.length} strategy templates shown · backtested on ${exchanges || "exchange"} data · sorted by gain`
              : " "}
          </div>
        </div>
        <div className="ranges" role="tablist">
          <button type="button" className={`range${engine === "all" ? " on" : ""}`} onClick={() => setEngine("all")}>
            All
          </button>
          {engines.map((e) => (
            <button key={e} type="button" className={`range${engine === e ? " on" : ""}`} onClick={() => setEngine(e)}>
              {engineLabel(e)}
            </button>
          ))}
        </div>
      </div>
      <ErrorBanner error={error} onRetry={reload} />
      {loading && !data && <Loading what="templates" />}
      {data && (
        <div className="cards">
          {shown.map((t) => (
            <TemplateCard key={t.name} t={t} />
          ))}
          {shown.length === 0 && <div className="msg">No templates published yet.</div>}
        </div>
      )}
    </>
  );
}

function TemplateCard({ t }: { t: TemplateSummary }) {
  const gain = t.metrics.gain;
  // A backtest the engine cut short is an account that got liquidated inside
  // the window; its gain is the balance before the wipe, not a result.
  const wiped = wipedOut(t.metrics);
  return (
    <Link to={`/configs/${encodeURIComponent(t.name)}`} className="tcard">
      <div className="head">
        <div className="name">{t.name}</div>
        {wiped && <Badge>liquidated in backtest</Badge>}
        <Badge>{engineLabel(t.engine)}</Badge>
      </div>
      <div className="mid">
        <div className="stats">
          <div>
            <div className="k">Gain</div>
            <div className="v" style={{ color: !wiped && gain != null && gain >= 1 ? "var(--pnl)" : "var(--pnl-neg)" }}>
              {wiped ? "wiped out" : fmtGain(gain, 0)}
            </div>
          </div>
          <div>
            <div className="k">Max DD</div>
            <div className="v">{fmtMetric("drawdown_worst", t.metrics.drawdown_worst)}</div>
          </div>
          <div>
            <div className="k">Sharpe</div>
            <div className="v">{fmtMetric("sharpe_ratio", t.metrics.sharpe_ratio)}</div>
          </div>
        </div>
        <TemplateSpark name={t.name} up={gain == null || gain >= 1} />
      </div>
      <Chips items={t.coins} max={5} tight />
      <div className="hint">
        Backtest {t.start.slice(0, 7)} → {t.end.slice(0, 7)} · {t.exchange}
      </div>
    </Link>
  );
}

// The card's sparkline comes from the template's own file, fetched lazily per
// card so the index stays small.
function TemplateSpark({ name, up }: { name: string; up: boolean }) {
  const { data } = useLoad(() => staticData.template(name), `template:${name}`);
  if (!data) return <div style={{ width: 100, height: 30 }} />;
  return <Sparkline pts={data.points.map((p) => ({ ts: p.ts, v: p.equity }))} up={up} w={100} h={30} />;
}
