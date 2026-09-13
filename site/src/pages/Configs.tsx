import { useMemo, useState } from "react";
import { Link } from "react-router-dom";
import { api } from "../api/client";
import { useLoad } from "../api/hooks";
import { useAuth } from "../auth/AuthProvider";
import {
  Badge,
  Chips,
  ErrorBanner,
  Loading,
  Sparkline,
  TemplateTags,
  engineLabel,
  templateTitle,
} from "../components/ui";
import { staticData, type TemplateSummary } from "../data/static";
import { useLang, useT } from "../i18n/locale";
import { fmtGain, fmtMetric, wipedOut } from "./metrics";

export function Configs() {
  const t = useT();
  const { session } = useAuth();
  const { data, error, loading, reload } = useLoad(() => staticData.templates(), "templates");
  // Whose catalogue this is: the operator's account is offered every template,
  // anyone else (a member, a visitor, a session the API knows no account for)
  // the public ones.
  const me = useLoad(() => (session ? api.me() : Promise.resolve(null)), session ? "me" : "me:none");
  const operator = me.data?.role === "operator";
  const [engine, setEngine] = useState<string>("all");

  const offered = useMemo(
    () => (data ?? []).filter((tpl) => operator || tpl.audience !== "operator"),
    [data, operator],
  );
  const engines = useMemo(() => Array.from(new Set(offered.map((tpl) => tpl.engine))).sort().reverse(), [offered]);
  const shown = useMemo(() => {
    const list = offered.filter((tpl) => engine === "all" || tpl.engine === engine);
    return list.sort((a, b) => (b.metrics.gain ?? 0) - (a.metrics.gain ?? 0));
  }, [offered, engine]);
  const exchanges = Array.from(new Set(shown.map((tpl) => tpl.exchange))).join(", ");

  return (
    <>
      <div className="page-head">
        <div>
          <h1>{t.configs.list.title}</h1>
          <div className="sub">{data ? t.configs.list.lead(shown.length, offered.length, exchanges) : " "}</div>
        </div>
        <div className="ranges" role="tablist">
          <button type="button" className={`range${engine === "all" ? " on" : ""}`} onClick={() => setEngine("all")}>
            {t.configs.list.allEngines}
          </button>
          {engines.map((e) => (
            <button key={e} type="button" className={`range${engine === e ? " on" : ""}`} onClick={() => setEngine(e)}>
              {engineLabel(e)}
            </button>
          ))}
        </div>
      </div>
      <ErrorBanner error={error} onRetry={reload} />
      {loading && !data && <Loading what={t.configs.list.templates} />}
      {data && (
        <div className="cards">
          {shown.map((tpl) => (
            <TemplateCard key={tpl.name} tpl={tpl} />
          ))}
          {shown.length === 0 && <div className="msg">{t.configs.list.empty}</div>}
        </div>
      )}
    </>
  );
}

function TemplateCard({ tpl }: { tpl: TemplateSummary }) {
  const t = useT();
  const { lang } = useLang();
  const gain = tpl.metrics.gain;
  // A backtest the engine cut short is an account that got liquidated inside
  // the window; its gain is the balance before the wipe, not a result.
  const wiped = wipedOut(tpl.metrics);
  return (
    <Link to={`/configs/${encodeURIComponent(tpl.name)}`} className="tcard">
      <div className="head">
        <div className="name">{templateTitle(tpl, lang)}</div>
        {tpl.audience === "operator" && <Badge>{t.configs.internalBadge}</Badge>}
        {wiped && <Badge>{t.configs.liquidatedBadge}</Badge>}
        <TemplateTags tpl={tpl} />
        <Badge>{engineLabel(tpl.engine)}</Badge>
      </div>
      <div className="mid">
        <div className="stats">
          <div>
            <div className="k">{t.configs.metric.gain}</div>
            <div className="v" style={{ color: !wiped && gain != null && gain >= 1 ? "var(--pnl)" : "var(--pnl-neg)" }}>
              {wiped ? t.configs.wipedOut : fmtGain(gain, 0)}
            </div>
          </div>
          <div>
            <div className="k">{t.configs.list.maxDd}</div>
            <div className="v">{fmtMetric("drawdown_worst", tpl.metrics.drawdown_worst)}</div>
          </div>
          <div>
            <div className="k">{t.configs.metric.sharpe_ratio}</div>
            <div className="v">{fmtMetric("sharpe_ratio", tpl.metrics.sharpe_ratio)}</div>
          </div>
        </div>
        <TemplateSpark name={tpl.name} up={gain == null || gain >= 1} />
      </div>
      <Chips items={tpl.coins} max={5} tight />
      <div className="hint">
        <span className="mono">{tpl.name}</span>
        <br />
        {t.configs.list.backtestRange(tpl.start.slice(0, 7), tpl.end.slice(0, 7), tpl.exchange)}
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
