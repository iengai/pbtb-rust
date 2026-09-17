import { useEffect, useMemo, useState } from "react";
import { Link } from "react-router-dom";
import { api } from "../api/client";
import { useLoad } from "../api/hooks";
import { useAuth } from "../auth/AuthProvider";
import {
  Badge,
  Chips,
  ErrorBanner,
  type HolderBot,
  HolderBots,
  Loading,
  Sparkline,
  TemplateTags,
  engineLabel,
  templateTitle,
} from "../components/ui";
import { currentTemplate } from "../chart/showcase";
import { SLOW_EDGE_MS, isRetired, staticData, type TemplateSummary } from "../data/static";
import { useLang, useT } from "../i18n/locale";
import { SORTS, SORT_KEYS, type SortKey, fmtGain, fmtMetric, sortTemplates, wipedOut } from "./metrics";

// The two classes of the catalogue: published (offered to everyone) and
// retired (`audience: operator`, the operator's account alone).
type Catalogue = "published" | "retired";

export function Configs() {
  const t = useT();
  const { session } = useAuth();
  const { data, error, loading, reload } = useLoad(() => staticData.templates(), "templates");
  // Whose catalogue this is: the operator's account is offered the retired
  // templates under their own tab, anyone else (a member, a visitor, a session
  // the API knows no account for) the published ones alone.
  const me = useLoad(() => (session ? api.me() : Promise.resolve(null)), session ? "me" : "me:none");
  const operator = me.data?.role === "operator";
  const [tab, setTab] = useState<Catalogue>("published");
  const [engine, setEngine] = useState<string>("all");
  // The key the list is sorted by, and whether its own direction is reversed:
  // a second press on the chosen key turns the list around.
  const [sort, setSort] = useState<{ key: SortKey; flip: boolean }>({ key: "gain", flip: false });
  const catalogue: Catalogue = operator ? tab : "published";
  // Each template's audience: the operator's tabs read it live from the API,
  // so a retire or publish shows at once; everyone else's list reads the public
  // overlay the switch rewrites, and the committed snapshot while there is none.
  const live = useLoad(
    () => (operator ? api.listTemplates() : Promise.resolve(null)),
    operator ? "templates:live" : "templates:live:none",
  );
  const overlay = useLoad(
    () => (operator ? Promise.resolve(null) : staticData.templatesPublished()),
    operator ? "templates:published:none" : "templates:published",
  );
  const liveRetired = useMemo(
    () => new Map((live.data?.templates ?? []).map((tpl) => [tpl.name, tpl.audience === "operator"])),
    [live.data],
  );

  const offered = useMemo(
    () =>
      (data ?? []).filter(
        (tpl) =>
          (liveRetired.get(tpl.name) ?? isRetired(tpl, overlay.data ?? null)) === (catalogue === "retired"),
      ),
    [data, catalogue, liveRetired, overlay.data],
  );
  const engines = useMemo(() => Array.from(new Set(offered.map((tpl) => tpl.engine))).sort().reverse(), [offered]);
  const shown = useMemo(
    () =>
      sortTemplates(
        offered.filter((tpl) => engine === "all" || tpl.engine === engine),
        sort.key,
        sort.flip,
      ),
    [offered, engine, sort],
  );
  // The bots holding each template now. The account's own come from the API,
  // one read per bot since the list carries no config; the showcase's from the
  // public files, left out for the operator, whose own bots they are.
  const mine = useLoad(
    async () => {
      if (!session) return [];
      const { bots } = await api.listBots();
      // A bot whose read fails (deleted since the list, a 5xx) is left out alone.
      const read = await Promise.all(bots.map((b) => api.getBot(b.bot_id).catch(() => null)));
      return read.filter((b) => b != null);
    },
    session ? "bots:configs" : "bots:configs:none",
  );
  const showcase = useLoad(() => staticData.showcaseBots(), "showcase:bots");
  // The showcase bots are waited on no longer than the overlay is: past that
  // the list draws and their row fills in when they come.
  const [showcaseLate, setShowcaseLate] = useState(false);
  useEffect(() => {
    const timer = setTimeout(() => setShowcaseLate(true), SLOW_EDGE_MS);
    return () => clearTimeout(timer);
  }, []);
  const holders = useMemo(() => {
    const by = new Map<string, { mine: HolderBot[]; showcase: HolderBot[] }>();
    const at = (name: string) => by.get(name) ?? by.set(name, { mine: [], showcase: [] }).get(name)!;
    for (const b of mine.data ?? []) {
      if (!b.config) continue;
      at(b.config.template_name).mine.push({
        key: b.bot_id,
        name: b.name,
        to: `/bots/${encodeURIComponent(b.bot_id)}`,
        phase: b.phase,
        mine: true,
      });
    }
    // A session's role is not known until `GET /me` settles; one it fails for
    // is a visitor's.
    const visitor = session ? !me.loading && !operator : true;
    for (const b of visitor ? (showcase.data ?? []) : []) {
      const name = currentTemplate(b);
      if (name) at(name).showcase.push({ key: b.id, name: b.name, to: `/p/bots/${encodeURIComponent(b.id)}`, mine: false });
    }
    return by;
  }, [mine.data, showcase.data, operator, session, me.loading]);
  const exchanges = Array.from(new Set(shown.map((tpl) => tpl.exchange))).join(", ");
  // The list is drawn once what decides it has answered: which templates it
  // holds (the role, then the live audience or the overlay) and the showcase
  // bots a visitor's cards name, up to the slow-edge deadline. Drawn from the
  // snapshot first, a retired template would show and then leave, moving every
  // card after it. The account's own bots are not waited on; their row fills
  // in under the cards.
  const settled =
    data != null &&
    (!session || !me.loading) &&
    (operator ? !live.loading : !overlay.loading) &&
    (operator || !showcase.loading || showcaseLate);

  return (
    <>
      <div className="page-head">
        <div>
          <h1>{t.configs.list.title}</h1>
          <div className="sub">{settled ? t.configs.list.lead(shown.length, offered.length, exchanges) : " "}</div>
        </div>
        <div style={{ display: "flex", gap: 10, flexWrap: "wrap" }}>
          {operator && (
            <div className="ranges" role="tablist">
              {(["published", "retired"] as const).map((c) => (
                <button
                  key={c}
                  type="button"
                  role="tab"
                  aria-selected={tab === c}
                  className={`range${tab === c ? " on" : ""}`}
                  onClick={() => {
                    setTab(c);
                    setEngine("all");
                  }}
                >
                  {t.configs.list.tabs[c]}
                </button>
              ))}
            </div>
          )}
          <div className="ranges">
            <button type="button" className={`range${engine === "all" ? " on" : ""}`} onClick={() => setEngine("all")}>
              {t.configs.list.allEngines}
            </button>
            {engines.map((e) => (
              <button key={e} type="button" className={`range${engine === e ? " on" : ""}`} onClick={() => setEngine(e)}>
                {engineLabel(e)}
              </button>
            ))}
          </div>
          <div className="ranges" role="group" aria-label={t.configs.list.sortBy}>
            {SORT_KEYS.map((key) => {
              const on = sort.key === key;
              return (
                <button
                  key={key}
                  type="button"
                  aria-pressed={on}
                  className={`range${on ? " on" : ""}`}
                  onClick={() => setSort({ key, flip: on && !sort.flip })}
                >
                  {t.configs.list.sorts[key]}
                  {on && (SORTS[key].desc !== sort.flip ? " ↓" : " ↑")}
                </button>
              );
            })}
          </div>
        </div>
      </div>
      <ErrorBanner error={error} onRetry={reload} />
      {(loading || (data && !settled)) && <Loading what={t.configs.list.templates} />}
      {settled && (
        <div className="cards">
          {shown.map((tpl) => (
            <TemplateCard key={tpl.name} tpl={tpl} holders={holders.get(tpl.name)} />
          ))}
          {shown.length === 0 && <div className="msg">{t.configs.list.empty[catalogue]}</div>}
        </div>
      )}
    </>
  );
}

function TemplateCard({
  tpl,
  holders,
}: {
  tpl: TemplateSummary;
  holders: { mine: HolderBot[]; showcase: HolderBot[] } | undefined;
}) {
  const t = useT();
  const { lang } = useLang();
  const gain = tpl.metrics.gain;
  // A backtest the engine cut short is an account that got liquidated inside
  // the window; its gain is the balance before the wipe, not a result.
  const wiped = wipedOut(tpl.metrics);
  return (
    <div className="tcard">
      <div className="head">
        <div className="name">
          <Link to={`/configs/${encodeURIComponent(tpl.name)}`}>{templateTitle(tpl, lang)}</Link>
        </div>
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
      {holders && holders.mine.length > 0 && <HolderBots bots={holders.mine} label={t.configs.list.myBots} />}
      {holders && holders.showcase.length > 0 && (
        <HolderBots bots={holders.showcase} label={t.configs.list.showcaseBots} />
      )}
      <div className="hint">
        <span className="mono">{tpl.name}</span>
        <br />
        {t.configs.list.backtestRange(tpl.start.slice(0, 7), tpl.end.slice(0, 7), tpl.exchange)}
      </div>
    </div>
  );
}

// The card's sparkline comes from the template's own file, fetched lazily per
// card so the index stays small.
function TemplateSpark({ name, up }: { name: string; up: boolean }) {
  const { data } = useLoad(() => staticData.template(name), `template:${name}`);
  if (!data) return <div style={{ width: 100, height: 30 }} />;
  return <Sparkline pts={data.points.map((p) => ({ ts: p.ts, v: p.equity }))} up={up} w={100} h={30} />;
}
