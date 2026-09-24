import { useState } from "react";
import { useNavigate, useParams } from "react-router-dom";
import { api } from "../api/client";
import { useAction, useLoad } from "../api/hooks";
import { useAuth } from "../auth/AuthProvider";
import {
  Badge,
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
import { isRetired, staticData, type TemplateBacktest } from "../data/static";
import { useLang, useT } from "../i18n/locale";
import { ConfigChart } from "./ConfigChart";
import { fmtGain, fmtMetric, metricRows, tradedLabels, wipedOut } from "./metrics";

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
  const [applying, setApplying] = useState(false);
  const [switching, setSwitching] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);

  const sides = data ? Array.from(new Set(data.strategies.map((s) => s.side))) : [];
  // Metric labels are keyed by passivbot's metric key; a key the catalog does
  // not know is shown as it comes out of analysis.json.
  const metricLabels: Record<string, string> = t.configs.metric;
  const styleNames: Record<string, string> = t.configs.style;
  // One row is the template's own run again; the table is worth showing from two.
  const hasProfile = (data?.capital_profile?.rows.length ?? 0) > 1;

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
                {sides.map((s) => (
                  <Badge key={s}>{s}</Badge>
                ))}
                {wipedOut(data.metrics) && <Badge>{t.configs.liquidatedBadge}</Badge>}
              </div>
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
          {data.candle_minutes != null && data.candle_minutes > 1 && (
            <div className="hint" style={{ marginTop: 6 }}>
              {t.configs.detail.candleNote(data.candle_minutes / 60)}
              {data.exchange === "hyperliquid" && ` ${t.configs.detail.copyNote}`}
            </div>
          )}

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
                  <div className="k">{t.configs.detail.positions}</div>
                  <div>
                    {data.positions ? t.configs.positions[data.positions] : "—"}
                    {data.positions && <div className="hint">{t.configs.detail.positionsHint[data.positions]}</div>}
                  </div>
                  <div className="k">{t.configs.detail.style}</div>
                  <div>{data.style ? (styleNames[data.style] ?? data.style) : "—"}</div>
                  <div className="k">{t.configs.detail.generation}</div>
                  <div>{data.generation != null ? t.configs.generation(data.generation) : "—"}</div>
                  <div className="k">{t.configs.detail.engine}</div>
                  <div>{t.configs.detail.engineValue(engineLabel(data.engine))}</div>
                  <div className="k">{t.configs.detail.minCapital}</div>
                  <div>
                    {fmtCap(data.starting_balance ?? 0)}
                    <div className="hint">
                      {t.configs.detail.minCapitalHint}
                      {hasProfile && ` ${t.configs.detail.minCapitalProfileHint}`}
                    </div>
                  </div>
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
          {hasProfile && data.capital_profile && (
            <div className="card" style={{ marginTop: 16 }}>
              <div className="card-title" style={{ marginBottom: 4 }}>
                {t.configs.profile.title}
              </div>
              <div className="hint" style={{ marginBottom: 10 }}>
                {t.configs.profile.hint}
              </div>
              <div className="table profile">
                <div className="th">
                  <span>{t.configs.profile.balance}</span>
                  <span>{t.configs.metric.gain}</span>
                  <span>{t.configs.list.maxDd}</span>
                  <span>{t.configs.profile.traded}</span>
                </div>
                {data.capital_profile.rows.map((row) => (
                  <div key={row.balance} className="tr">
                    <span className="tnum">{fmtCap(row.balance)}</span>
                    <span className="tnum">{fmtGain(row.gain ?? undefined, 0)}</span>
                    <span className="tnum">{fmtMetric("drawdown_worst", row.drawdown_worst ?? undefined)}</span>
                    {/* No cap: the column is what the table is for, and a
                        coin behind a +N is one the reader cannot check. */}
                    <Chips items={tradedLabels(row.coins)} tight />
                  </div>
                ))}
              </div>
            </div>
          )}
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
