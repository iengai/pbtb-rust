import { useState } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";
import { api } from "../api/client";
import { useAction, useLoad } from "../api/hooks";
import { useAuth } from "../auth/AuthProvider";
import {
  Badge,
  CapitalPills,
  Chips,
  Crumbs,
  ErrorBanner,
  Loading,
  Modal,
  TemplateTags,
  engineLabel,
  templateTitle,
} from "../components/ui";
import { fmtCap } from "../chart/showcase";
import { isRetired, paramFamilies, sameParams, staticData, type TemplateBacktest } from "../data/static";
import { useLang, useT } from "../i18n/locale";
import { ConfigChart } from "./ConfigChart";
import { fmtGain, fmtMetric, metricRows, wipedOut } from "./metrics";

export function ConfigDetail() {
  const { name = "" } = useParams();
  const t = useT();
  const { lang } = useLang();
  const { session } = useAuth();
  const navigate = useNavigate();
  const { data, error, loading, reload } = useLoad(() => staticData.template(name), `template:${name}`);
  const me = useLoad(() => (session ? api.me() : Promise.resolve(null)), session ? "me" : "me:none");
  const operator = me.data?.role === "operator";
  // The operator reads the audience live, so the switch below shows its result
  // at once; everyone else reads the public overlay the switch rewrites, and
  // the published backtest's own mark while there is none.
  const live = useLoad(
    () => (operator ? api.listTemplates() : Promise.resolve(null)),
    operator ? "templates:live" : "templates:live:none",
  );
  const overlay = useLoad(
    () => (operator ? Promise.resolve(null) : staticData.templatesPublished()),
    operator ? "templates:published:none" : "templates:published",
  );
  const liveAudience = live.data?.templates.find((tpl) => tpl.name === name)?.audience;
  // A retired template is applied by the operator's account alone; the page
  // shows everyone the backtest and offers the apply to no one else.
  const retired = liveAudience
    ? liveAudience === "operator"
    : data
      ? isRetired(data, overlay.data ?? null)
      : false;
  const canApply = !retired || operator;
  // The templates trading this one's parameter set at another capital, among
  // those the viewer's catalogue lists: the operator's holds the retired ones.
  const index = useLoad(() => staticData.templates(), "templates");
  const retiredNow = (tpl: { name: string; audience?: "operator" | null }) => {
    const audience = live.data?.templates.find((other) => other.name === tpl.name)?.audience;
    return audience ? audience === "operator" : isRetired(tpl, overlay.data ?? null);
  };
  const listed = (index.data ?? []).filter((tpl) => operator || !retiredNow(tpl));
  const siblings = data ? sameParams(data, listed) : [];
  const family = paramFamilies(index.data ?? []).get(data?.params_sha ?? "");
  const [applying, setApplying] = useState(false);
  const [switching, setSwitching] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);

  const sides = data ? Array.from(new Set(data.strategies.map((s) => s.side))) : [];
  // Metric labels are keyed by passivbot's metric key; a key the catalog does
  // not know is shown as it comes out of analysis.json.
  const metricLabels: Record<string, string> = t.configs.metric;
  const styleNames: Record<string, string> = t.configs.style;

  return (
    <>
      <Crumbs
        items={[
          { to: "/configs", label: t.common.nav.configs },
          { label: data ? templateTitle(data, lang) : <span className="mono">{name}</span> },
        ]}
      />
      <ErrorBanner error={error} onRetry={reload} />
      {notice && (
        <div className="banner info" style={{ marginBottom: 14 }}>
          <div className="body">{notice}</div>
          <button type="button" className="btn ghost act" style={{ height: 30 }} onClick={() => setNotice(null)}>
            {t.common.dismiss}
          </button>
        </div>
      )}
      {loading && !data && <Loading what={t.configs.detail.template} />}
      {data && (
        <>
          <div className="page-head top">
            <div style={{ minWidth: 0 }}>
              <div style={{ display: "flex", alignItems: "center", gap: 10, flexWrap: "wrap" }}>
                <h1>{templateTitle(data, lang)}</h1>
                <TemplateTags tpl={data} />
                {retired && <Badge>{t.configs.retiredBadge}</Badge>}
                <Badge>{engineLabel(data.engine)}</Badge>
                {sides.map((s) => (
                  <Badge key={s}>{s}</Badge>
                ))}
                {wipedOut(data.metrics) && <Badge>{t.configs.liquidatedBadge}</Badge>}
              </div>
              {family != null && siblings.length > 0 && (
                <div style={{ marginTop: 8 }}>
                  <CapitalPills
                    members={[data, ...siblings].sort((a, b) => (a.starting_balance ?? 0) - (b.starting_balance ?? 0))}
                    current={data.name}
                    family={family}
                    label={t.configs.list.sameParams}
                  />
                </div>
              )}
              <div className="sub" style={{ marginTop: 4 }}>
                <span className="mono">{data.name}</span> ·{" "}
                {t.configs.detail.lead(data.exchange, data.start, data.end, data.coins.length)}
                {wipedOut(data.metrics) && ` · ${t.configs.detail.liquidatedNote}`}
                {retired && ` · ${t.configs.detail.retiredNote}`}
              </div>
            </div>
            <div className="btn-row">
              {/* Only a template the API lists can be switched: an archived one is in no catalogue. */}
              {operator && liveAudience && (
                <button type="button" className="btn" onClick={() => setSwitching(true)}>
                  {retired ? t.configs.audience.publish : t.configs.audience.retire}
                </button>
              )}
              {canApply && (
                <button
                  type="button"
                  className="btn primary"
                  onClick={() => (session ? setApplying(true) : navigate("/"))}
                >
                  {t.configs.detail.applyCta}
                </button>
              )}
            </div>
          </div>

          <ConfigChart key={data.name} template={data} />

          <div className="two-col">
            <div className="stack">
              <div className="card">
                <div className="card-title sm">{t.configs.detail.aboutTitle}</div>
                <div className="desc">{data.description || t.configs.detail.noDescription}</div>
              </div>
              <div className="card">
                <div className="card-title">{t.configs.detail.setupTitle}</div>
                <div className="kv wide">
                  <div className="k">{t.configs.detail.sides}</div>
                  <div>{sides.join(", ") || "—"}</div>
                  <div className="k">{t.configs.detail.coins}</div>
                  <div>
                    <Chips items={data.coins} />
                  </div>
                  <div className="k">{t.configs.detail.style}</div>
                  <div>{data.style ? (styleNames[data.style] ?? data.style) : "—"}</div>
                  <div className="k">{t.configs.detail.generation}</div>
                  <div>{data.generation != null ? t.configs.generation(data.generation) : "—"}</div>
                  <div className="k">{t.configs.detail.engine}</div>
                  <div>{t.configs.detail.engineValue(engineLabel(data.engine))}</div>
                  {siblings.length > 0 && (
                    <>
                      <div className="k">{t.configs.detail.sameParams}</div>
                      <div>
                        {siblings.map((tpl) => (
                          <div key={tpl.name}>
                            <Link to={`/configs/${encodeURIComponent(tpl.name)}`}>
                              {fmtCap(tpl.starting_balance ?? 0)}
                            </Link>{" "}
                            <span className="muted tnum">
                              {wipedOut(tpl.metrics) ? t.configs.wipedOut : fmtGain(tpl.metrics.gain, 0)} ·{" "}
                              {t.configs.list.maxDd} {fmtMetric("drawdown_worst", tpl.metrics.drawdown_worst)}
                            </span>
                            {retiredNow(tpl) && (
                              <>
                                {" "}
                                <Badge>{t.configs.retiredBadge}</Badge>
                              </>
                            )}
                          </div>
                        ))}
                        <div className="hint">{t.configs.detail.sameParamsHint}</div>
                      </div>
                    </>
                  )}
                </div>
              </div>
            </div>
            <div className="card">
              <div className="card-title" style={{ marginBottom: 4 }}>
                {t.configs.detail.metricsTitle}
              </div>
              <div className="hint" style={{ marginBottom: 8 }}>
                {t.configs.detail.metricsHint}
              </div>
              {metricRows(data.metrics).map((r) => (
                <div key={r.key} className="metric-row">
                  <span className="muted">{metricLabels[r.key] ?? r.key}</span>
                  <span className="tnum" style={{ fontWeight: 550 }}>
                    {r.value}
                  </span>
                </div>
              ))}
            </div>
          </div>
          {applying && (
            <ApplyDialog
              template={data}
              onClose={() => setApplying(false)}
              onDone={(msg) => {
                setApplying(false);
                setNotice(msg);
              }}
            />
          )}
          {switching && (
            <AudienceDialog
              template={data}
              retire={!retired}
              onClose={() => setSwitching(false)}
              onDone={(msg) => {
                setSwitching(false);
                setNotice(msg);
                live.reload();
              }}
            />
          )}
        </>
      )}
    </>
  );
}

function AudienceDialog({
  template,
  retire,
  onClose,
  onDone,
}: {
  template: TemplateBacktest;
  retire: boolean;
  onClose: () => void;
  onDone: (msg: string) => void;
}) {
  const t = useT();
  const { lang } = useLang();
  const action = useAction();
  const title = templateTitle(template, lang);
  const kind = retire ? "retire" : "publish";
  return (
    <Modal title={t.configs.audience.title[kind](title)} onClose={onClose}>
      <div className="banner">
        <div className="body">
          {t.configs.audience.body[kind]} {t.configs.audience.publicNote}
        </div>
      </div>
      <ErrorBanner error={action.error} onDismiss={action.clear} />
      <div className="actions">
        <button type="button" className="btn ghost" onClick={onClose}>
          {t.common.cancel}
        </button>
        <button
          type="button"
          className={`btn primary${retire ? " danger solid" : ""}`}
          disabled={action.busy}
          onClick={() =>
            void action.run(async () => {
              await api.setTemplateAudience(template.name, retire ? "operator" : "everyone");
              onDone(t.configs.audience.done[kind](title));
            })
          }
        >
          {t.configs.audience[kind]}
        </button>
      </div>
    </Modal>
  );
}

function ApplyDialog({
  template,
  onClose,
  onDone,
}: {
  template: TemplateBacktest;
  onClose: () => void;
  onDone: (msg: string) => void;
}) {
  const t = useT();
  const { lang } = useLang();
  const bots = useLoad(() => api.listBots(), "bots");
  const [botId, setBotId] = useState("");
  const [confirm, setConfirm] = useState(false);
  const action = useAction();
  const chosen = bots.data?.bots.find((b) => b.bot_id === botId);
  return (
    <Modal title={t.configs.apply.title(templateTitle(template, lang))} onClose={onClose}>
      <ErrorBanner error={bots.error} onRetry={bots.reload} />
      <div className="field">
        <label>{t.configs.apply.botLabel}</label>
        <select className="select" value={botId} onChange={(e) => setBotId(e.target.value)} disabled={!bots.data}>
          <option value="">{bots.data ? t.configs.apply.chooseBot : t.common.loading}</option>
          {bots.data?.bots.map((b) => (
            <option key={b.bot_id} value={b.bot_id}>
              {b.name} · {b.exchange}
            </option>
          ))}
        </select>
      </div>
      {chosen && (
        <div className="banner">
          <div className="body">{t.configs.apply.warning(chosen.name)}</div>
        </div>
      )}
      <ErrorBanner error={action.error} onDismiss={action.clear} />
      <div className="actions">
        <button type="button" className="btn ghost" onClick={onClose}>
          {t.common.cancel}
        </button>
        {!confirm ? (
          <button type="button" className="btn primary" disabled={!chosen} onClick={() => setConfirm(true)}>
            {t.configs.apply.proceed}
          </button>
        ) : (
          <button
            type="button"
            className="btn primary"
            disabled={action.busy}
            onClick={() =>
              void action.run(async () => {
                await api.applyTemplate(botId, template.name);
                onDone(t.configs.apply.done(templateTitle(template, lang), chosen?.name ?? ""));
              })
            }
          >
            {t.configs.apply.applyTo(chosen?.name ?? "")}
          </button>
        )}
      </div>
    </Modal>
  );
}
