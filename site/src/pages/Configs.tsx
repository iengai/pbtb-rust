import { type CSSProperties, useMemo, useState } from "react";
import { Link } from "react-router-dom";
import { api } from "../api/client";
import { useLoad } from "../api/hooks";
import { useAuth } from "../auth/AuthProvider";
import {
  Badge,
  CapitalPills,
  Chips,
  ErrorBanner,
  Loading,
  Sparkline,
  TemplateTags,
  engineLabel,
  familyColor,
  templateTitle,
} from "../components/ui";
import { isRetired, paramFamilies, sameParams, staticData, type TemplateSummary } from "../data/static";
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
  const families = useMemo(() => paramFamilies(data ?? []), [data]);
  // The parameter set of the card pointed at: its other capitals light up too.
  const [pointed, setPointed] = useState<string | null>(null);
  const exchanges = Array.from(new Set(shown.map((tpl) => tpl.exchange))).join(", ");

  return (
    <>
      <div className="page-head">
        <div>
          <h1>{t.configs.list.title}</h1>
          <div className="sub">{data ? t.configs.list.lead(shown.length, offered.length, exchanges) : " "}</div>
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
      {loading && !data && <Loading what={t.configs.list.templates} />}
      {data && (
        <div className="cards">
          {shown.map((tpl) => (
            <TemplateCard
              key={tpl.name}
              tpl={tpl}
              siblings={sameParams(tpl, offered)}
              family={families.get(tpl.params_sha ?? "")}
              kin={pointed != null && pointed === tpl.params_sha}
              onPoint={setPointed}
            />
          ))}
          {shown.length === 0 && <div className="msg">{t.configs.list.empty[catalogue]}</div>}
        </div>
      )}
    </>
  );
}

function TemplateCard({
  tpl,
  siblings,
  family,
  kin,
  onPoint,
}: {
  tpl: TemplateSummary;
  siblings: TemplateSummary[];
  family: number | undefined;
  kin: boolean;
  onPoint: (params: string | null) => void;
}) {
  const t = useT();
  const { lang } = useLang();
  const gain = tpl.metrics.gain;
  // A backtest the engine cut short is an account that got liquidated inside
  // the window; its gain is the balance before the wipe, not a result.
  const wiped = wipedOut(tpl.metrics);
  // A set the list shows at this capital alone wears no colour.
  const set = family != null && siblings.length > 0;
  const colour = family ?? 0;
  const members = [tpl, ...siblings].sort((a, b) => (a.starting_balance ?? 0) - (b.starting_balance ?? 0));
  return (
    <div
      className={`tcard${set ? " fam" : ""}${set && kin ? " kin" : ""}`}
      style={set ? ({ "--fam": familyColor(colour) } as CSSProperties) : undefined}
      onMouseEnter={() => set && onPoint(tpl.params_sha ?? null)}
      onMouseLeave={() => set && onPoint(null)}
      onFocus={() => set && onPoint(tpl.params_sha ?? null)}
      onBlur={() => set && onPoint(null)}
    >
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
      {set && (
        <CapitalPills
          members={members}
          current={tpl.name}
          family={colour}
          label={t.configs.list.sameParams}
        />
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
