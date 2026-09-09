import { useState } from "react";
import { useNavigate, useParams } from "react-router-dom";
import { api } from "../api/client";
import { useAction, useLoad } from "../api/hooks";
import { useAuth } from "../auth/AuthProvider";
import { EquityChart } from "../chart/EquityChart";
import { Badge, Chips, Crumbs, ErrorBanner, Loading, Modal, engineLabel } from "../components/ui";
import { staticData, type TemplateBacktest } from "../data/static";
import { metricRows, wipedOut } from "./metrics";

export function ConfigDetail() {
  const { name = "" } = useParams();
  const { session } = useAuth();
  const navigate = useNavigate();
  const { data, error, loading, reload } = useLoad(() => staticData.template(name), `template:${name}`);
  const [applying, setApplying] = useState(false);
  const [notice, setNotice] = useState<string | null>(null);

  const sides = data ? Array.from(new Set(data.strategies.map((s) => s.side))) : [];

  return (
    <>
      <Crumbs items={[{ to: "/configs", label: "Configs" }, { label: <span className="mono">{name}</span> }]} />
      <ErrorBanner error={error} onRetry={reload} />
      {notice && (
        <div className="banner info" style={{ marginBottom: 14 }}>
          <div className="body">{notice}</div>
          <button type="button" className="btn ghost act" style={{ height: 30 }} onClick={() => setNotice(null)}>
            Dismiss
          </button>
        </div>
      )}
      {loading && !data && <Loading what="template" />}
      {data && (
        <>
          <div className="page-head top">
            <div style={{ minWidth: 0 }}>
              <div style={{ display: "flex", alignItems: "center", gap: 10, flexWrap: "wrap" }}>
                <h1 className="mono">{data.name}</h1>
                <Badge>{engineLabel(data.engine)}</Badge>
                {sides.map((s) => (
                  <Badge key={s}>{s}</Badge>
                ))}
                {wipedOut(data.metrics) && <Badge>liquidated in backtest</Badge>}
              </div>
              <div className="sub" style={{ marginTop: 4 }}>
                Tuned on {data.exchange} data · backtest {data.start} → {data.end} · {data.coins.length} coin
                {data.coins.length === 1 ? "" : "s"}
                {wipedOut(data.metrics) &&
                  " · the account was liquidated before the window ended; the metrics describe the run up to that point"}
              </div>
            </div>
            <button
              type="button"
              className="btn primary"
              onClick={() => (session ? setApplying(true) : navigate("/"))}
            >
              Apply to a bot…
            </button>
          </div>

          <div className="card tight" style={{ marginBottom: 18 }}>
            <div className="card-title" style={{ marginBottom: 8, padding: "0 4px" }}>
              Backtest equity
            </div>
            <EquityChart points={data.points} />
          </div>

          <div className="two-col">
            <div className="stack">
              <div className="card">
                <div className="card-title sm">About this strategy</div>
                <div className="desc">{data.description || "No description."}</div>
              </div>
              <div className="card">
                <div className="card-title">Setup</div>
                <div className="kv wide">
                  <div className="k">Sides</div>
                  <div>{sides.join(", ") || "—"}</div>
                  <div className="k">Coins</div>
                  <div>
                    <Chips items={data.coins} />
                  </div>
                  <div className="k">Engine</div>
                  <div>passivbot {engineLabel(data.engine)} · runs on py or rs</div>
                </div>
              </div>
            </div>
            <div className="card">
              <div className="card-title" style={{ marginBottom: 4 }}>
                Backtest metrics
              </div>
              <div className="hint" style={{ marginBottom: 8 }}>
                USD figures from analysis.json
              </div>
              {metricRows(data.metrics).map((r) => (
                <div key={r.label} className="metric-row">
                  <span className="muted">{r.label}</span>
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
        </>
      )}
    </>
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
  const bots = useLoad(() => api.listBots(), "bots");
  const [botId, setBotId] = useState("");
  const [confirm, setConfirm] = useState(false);
  const action = useAction();
  const chosen = bots.data?.bots.find((b) => b.bot_id === botId);
  return (
    <Modal title={`Apply ${template.name}`} onClose={onClose}>
      <ErrorBanner error={bots.error} onRetry={bots.reload} />
      <div className="field">
        <label>Bot</label>
        <select className="select" value={botId} onChange={(e) => setBotId(e.target.value)} disabled={!bots.data}>
          <option value="">{bots.data ? "Choose a bot…" : "Loading…"}</option>
          {bots.data?.bots.map((b) => (
            <option key={b.bot_id} value={b.bot_id}>
              {b.name} · {b.exchange}
            </option>
          ))}
        </select>
      </div>
      {chosen && (
        <div className="banner">
          <div className="body">
            This replaces <b>{chosen.name}</b>'s strategy, sides, coins and risk settings with the template's.
            It applies on the bot's next start.
          </div>
        </div>
      )}
      <ErrorBanner error={action.error} onDismiss={action.clear} />
      <div className="actions">
        <button type="button" className="btn ghost" onClick={onClose}>
          Cancel
        </button>
        {!confirm ? (
          <button type="button" className="btn primary" disabled={!chosen} onClick={() => setConfirm(true)}>
            Continue
          </button>
        ) : (
          <button
            type="button"
            className="btn primary"
            disabled={action.busy}
            onClick={() =>
              void action.run(async () => {
                await api.applyTemplate(botId, template.name);
                onDone(`Applied ${template.name} to ${chosen?.name}; it takes effect on the next start.`);
              })
            }
          >
            Apply to {chosen?.name}
          </button>
        )}
      </div>
    </Modal>
  );
}
