import { useCallback, useEffect, useState } from "react";
import { useNavigate, useParams } from "react-router-dom";
import { api, isRetryConflict } from "../api/client";
import { useAction, useLoad } from "../api/hooks";
import type { BotDetail as Detail } from "../api/types";
import { staticData } from "../data/static";
import { ChartCaption, RangeSelector, ReturnChart, useRangeLabel } from "../chart/ReturnChart";
import {
  type BotReturnSeries,
  type ChartWindow,
  DEFAULT_RANGE,
  RANGES,
  fmtSignedPct,
  fmtUsdt,
  selectWindow,
} from "../chart/returnCurve";
import { Play, Stop, Trash } from "../components/icons";
import {
  Badge,
  Chips,
  Crumbs,
  ErrorBanner,
  Loading,
  Modal,
  PhasePill,
  Pill,
  Tile,
  engineLabel,
  relativeTime,
  runtimeLabel,
  templateTitle,
} from "../components/ui";
import { loadReturns } from "./returnsApi";
import { useLang, useT } from "../i18n/locale";

const POLL_MS = 15_000;

type Dialog = "stop" | "delete" | "template" | "risk" | "sides" | "runtime" | null;

export function BotDetail() {
  const { id = "" } = useParams();
  const navigate = useNavigate();
  const bot = useLoad(() => api.getBot(id), `bot:${id}`, POLL_MS);
  const [series, setSeries] = useState<BotReturnSeries | null | undefined>(undefined);
  const [range, setRange] = useState(DEFAULT_RANGE);
  const [dialog, setDialog] = useState<Dialog>(null);
  const action = useAction();
  const { lang } = useLang();
  const t = useT();
  const rangeLabel = useRangeLabel();
  const [notice, setNotice] = useState<string | null>(null);

  // A strategy carries its side as the API's raw enum value.
  const sideLabel = (side: string) =>
    side === "long" ? t.bots.long : side === "short" ? t.bots.short : side;

  useEffect(() => {
    if (!id) return;
    let alive = true;
    loadReturns(id)
      .catch(() => null)
      .then((s) => alive && setSeries(s));
    return () => {
      alive = false;
    };
  }, [id]);

  const close = useCallback(() => setDialog(null), []);
  const done = useCallback(
    (msg: string | null) => {
      setDialog(null);
      setNotice(msg);
      bot.reload();
    },
    [bot],
  );

  const d = bot.data;
  const win: ChartWindow | null = series ? selectWindow(series, range) : null;
  const ok = win?.kind === "ok" ? win : null;
  const rangeK = ok ? rangeLabel(ok.stats.label) : RANGES[range]!.k;

  const start = () =>
    action.run(async () => {
      const r = await api.startBot(id);
      done(
        r.status === "started"
          ? t.bots.detail.startRequested
          : r.status === "already_running"
            ? t.bots.detail.alreadyRunning
            : t.bots.detail.alreadyStarting,
      );
    });

  return (
    <>
      <Crumbs items={[{ to: "/bots", label: t.common.nav.bots }, { label: d?.name ?? id }]} />
      <ErrorBanner error={bot.error} onRetry={bot.reload} />
      <ErrorBanner
        error={action.error}
        onRetry={isRetryConflict(action.error) ? bot.reload : undefined}
        onDismiss={action.clear}
      />
      {notice && (
        <div className="banner info" style={{ marginBottom: 14 }}>
          <div className="body">{notice}</div>
          <button type="button" className="btn ghost act" style={{ height: 30 }} onClick={() => setNotice(null)}>
            {t.common.dismiss}
          </button>
        </div>
      )}
      {!d && bot.loading && <Loading what={t.bots.detail.loadingWhat} />}
      {d && (
        <>
          <div className="page-head top">
            <div>
              <div style={{ display: "flex", alignItems: "center", gap: 10, flexWrap: "wrap" }}>
                <h1 className="lg">{d.name}</h1>
                <PhasePill phase={d.phase} enabled={d.enabled} />
                <Badge>{d.exchange.toUpperCase()}</Badge>
              </div>
              <div className="sub" style={{ marginTop: 4 }}>
                {t.bots.detail.desiredRuntime(
                  <b style={{ color: "var(--text)", fontWeight: 550 }}>
                    {d.enabled ? t.bots.enabled : t.bots.disabled}
                  </b>,
                  <b style={{ color: "var(--text)", fontWeight: 550 }}>{runtimeLabel(d.runtime)}</b>,
                )}
                {d.observed_at
                  ? ` · ${t.bots.detail.taskObserved(relativeTime(d.observed_at, lang))}`
                  : ` · ${t.bots.detail.noTaskObserved}`}
              </div>
            </div>
            <div className="btn-row">
              <button type="button" className="btn" onClick={() => setDialog("stop")} disabled={action.busy}>
                <Stop />
                {t.bots.detail.stopBot}
              </button>
              <button type="button" className="btn primary" onClick={() => void start()} disabled={action.busy}>
                <Play />
                {t.bots.detail.runBot}
              </button>
            </div>
          </div>

          <div className="tiles" style={{ gridTemplateColumns: "repeat(auto-fit, minmax(140px, 1fr))" }}>
            <Tile
              k={t.bots.detail.returnTile(rangeK)}
              v={ok ? fmtSignedPct(ok.stats.ret) : "—"}
              tone={ok ? (ok.stats.ret >= 0 ? "up" : "down") : undefined}
            />
            <Tile
              k={t.bots.detail.netPnlTile(rangeK)}
              v={fmtUsdt(ok?.stats.pnl)}
              tone={ok?.stats.pnl != null ? (ok.stats.pnl >= 0 ? "up" : "down") : undefined}
            />
            <Tile
              k={t.bots.detail.maxDrawdownTile(rangeK)}
              v={ok ? `−${Math.abs(ok.stats.maxDrawdown).toFixed(1)}%` : "—"}
            />
            <Tile k={t.bots.detail.leverage} v={d.config?.leverage != null ? `${d.config.leverage}x` : "—"} />
            <Tile k={t.bots.detail.configSwitches} v={series?.config_switches?.length ?? "—"} />
          </div>

          <div className="card tight" style={{ marginBottom: 18 }}>
            <div className="card-title chart">
              <div>{t.bots.detail.cumulativeReturn}</div>
              <RangeSelector value={range} onChange={setRange} />
            </div>
            {series === undefined && <Loading what={t.bots.detail.loadingChart} />}
            {series === null && (
              <div className="msg">
                {t.bots.detail.noReturnData}
                <br />
                <span className="hint">{t.bots.detail.noReturnDataHint}</span>
              </div>
            )}
            {win && <ReturnChart window={win} />}
            <div className="legend">
              <span>
                <i className="swatch" style={{ background: "var(--pnl)" }} /> {t.bots.detail.cumulativeReturn}
              </span>
            </div>
            <div className="hint" style={{ marginTop: 4 }}>
              {t.bots.detail.switchDot}
              {ok?.caption && (
                <>
                  {" "}
                  <ChartCaption caption={ok.caption} />
                </>
              )}
            </div>
          </div>

          <div className="two-col">
            <div className="card">
              <div className="card-title">{t.bots.detail.configuration}</div>
              {d.config ? (
                <div className="kv">
                  <div className="k">{t.bots.detail.template}</div>
                  <div style={{ display: "flex", alignItems: "center", gap: 8, minWidth: 0 }}>
                    <span className="ellipsis" title={d.config.template_name}>
                      {templateTitle({ ...d.config, name: d.config.template_name }, lang)}
                    </span>
                    {d.config.template_version && <Badge>{engineLabel(d.config.template_version)}</Badge>}
                  </div>
                  <div className="k">{t.bots.detail.tunedOn}</div>
                  <div>{d.config.tuned_on ? t.bots.detail.tunedOnValue(d.config.tuned_on) : "—"}</div>
                  <div className="k">{t.bots.detail.strategy}</div>
                  <div>
                    {d.config.strategies.length
                      ? d.config.strategies
                          .map((s) => t.bots.detail.strategyEntry(s.name, sideLabel(s.side)))
                          .join(", ")
                      : "—"}
                  </div>
                  <div className="k">{t.bots.detail.sides}</div>
                  <div style={{ display: "flex", gap: 8 }}>
                    <Pill tone={d.config.sides.long ? "ok" : undefined}>
                      {t.bots.detail.sidePillLong(d.config.sides.long)}
                    </Pill>
                    <Pill tone={d.config.sides.short ? "ok" : undefined}>
                      {t.bots.detail.sidePillShort(d.config.sides.short)}
                    </Pill>
                  </div>
                  <div className="k">{t.bots.detail.riskLevel}</div>
                  <div className="tnum">
                    {d.config.risk ? t.bots.detail.riskValue(d.config.risk.long, d.config.risk.short) : "—"}
                  </div>
                  <div className="k">{t.bots.detail.coins}</div>
                  <div>
                    {d.config.coins ? (
                      <Chips items={Array.from(new Set([...d.config.coins.long, ...d.config.coins.short]))} />
                    ) : (
                      "—"
                    )}
                  </div>
                </div>
              ) : (
                <div className="muted" style={{ fontSize: 14 }}>
                  {t.bots.detail.noConfig}
                </div>
              )}
              <div className="btn-row" style={{ marginTop: 16 }}>
                <button type="button" className="btn" onClick={() => setDialog("template")}>
                  {t.bots.detail.changeConfig}
                </button>
                <button type="button" className="btn" onClick={() => setDialog("risk")} disabled={!d.config}>
                  {t.bots.detail.riskLevel}
                </button>
                <button type="button" className="btn" onClick={() => setDialog("sides")} disabled={!d.config}>
                  {t.bots.detail.sides}
                </button>
                <button type="button" className="btn" onClick={() => setDialog("runtime")}>
                  {t.bots.detail.runtime}
                </button>
              </div>
              <div className="hint" style={{ marginTop: 10 }}>
                {t.bots.detail.configHint}
              </div>
            </div>
            <div className="stack">
              <div className="card">
                <div className="card-title sm">{t.bots.detail.balance}</div>
                <div className="tnum" style={{ fontSize: 20, fontWeight: 640, color: "var(--muted)" }}>
                  —
                </div>
                <div className="hint" style={{ marginTop: 4 }}>
                  {t.bots.detail.balanceHint}
                </div>
              </div>
              <div className="card">
                <div className="card-title sm">{t.bots.detail.dangerZone}</div>
                <div className="btn-col">
                  <button
                    type="button"
                    className="btn"
                    disabled={action.busy}
                    onClick={() => void action.run(() => api.unstuck(id))}
                  >
                    {t.bots.detail.unstuck}
                  </button>
                  <button type="button" className="btn danger" onClick={() => setDialog("delete")}>
                    <Trash />
                    {t.bots.detail.deleteBot}
                  </button>
                </div>
                <div className="hint" style={{ marginTop: 10 }}>
                  {t.bots.detail.deleteHint}
                </div>
              </div>
            </div>
          </div>

          {dialog === "stop" && (
            <Modal title={t.bots.stopModal.title} onClose={close}>
              <div style={{ fontSize: 14 }}>{t.bots.stopModal.body}</div>
              <div className="actions">
                <button type="button" className="btn ghost" onClick={close}>
                  {t.common.cancel}
                </button>
                <button
                  type="button"
                  className="btn danger solid"
                  disabled={action.busy}
                  onClick={() =>
                    void action.run(async () => {
                      const r = await api.stopBot(id);
                      done(
                        r.status === "stopped"
                          ? t.bots.stopModal.stopped
                          : r.status === "not_running"
                            ? t.bots.stopModal.notRunning
                            : t.bots.stopModal.alreadyStopping,
                      );
                    })
                  }
                >
                  {t.bots.detail.stopBot}
                </button>
              </div>
            </Modal>
          )}
          {dialog === "delete" && <DeleteDialog bot={d} onClose={close} onDeleted={() => navigate("/bots")} />}
          {dialog === "template" && <TemplateDialog bot={d} onClose={close} onDone={done} />}
          {dialog === "risk" && <RiskDialog bot={d} onClose={close} onDone={done} />}
          {dialog === "sides" && <SidesDialog bot={d} onClose={close} onDone={done} />}
          {dialog === "runtime" && <RuntimeDialog bot={d} onClose={close} onDone={done} />}
        </>
      )}
    </>
  );
}

function DeleteDialog({ bot, onClose, onDeleted }: { bot: Detail; onClose: () => void; onDeleted: () => void }) {
  const [typed, setTyped] = useState("");
  const action = useAction();
  const t = useT();
  return (
    <Modal title={t.bots.detail.deleteBot} onClose={onClose}>
      <div style={{ fontSize: 14 }}>{t.bots.deleteModal.body(bot.name)}</div>
      <div className="mono" style={{ fontSize: 13, color: "var(--muted)", userSelect: "all" }}>
        {bot.bot_id}
      </div>
      <input
        className="input mono"
        value={typed}
        onChange={(e) => setTyped(e.target.value)}
        placeholder={t.bots.deleteModal.placeholder}
        autoFocus
      />
      <ErrorBanner error={action.error} onDismiss={action.clear} />
      <div className="actions">
        <button type="button" className="btn ghost" onClick={onClose}>
          {t.common.cancel}
        </button>
        <button
          type="button"
          className="btn danger solid"
          disabled={typed !== bot.bot_id || action.busy}
          onClick={() =>
            void action.run(async () => {
              await api.deleteBot(bot.bot_id);
              onDeleted();
            })
          }
        >
          {t.common.delete}
        </button>
      </div>
    </Modal>
  );
}

function TemplateDialog({
  bot,
  onClose,
  onDone,
}: {
  bot: Detail;
  onClose: () => void;
  onDone: (msg: string) => void;
}) {
  const list = useLoad(() => api.listTemplates(), "templates");
  // The API lists template ids; the titles live with the published backtests,
  // so an id the catalogue does not carry still shows, as itself.
  const catalog = useLoad(() => staticData.templates(), "templates:titles");
  const [name, setName] = useState("");
  const [confirm, setConfirm] = useState(false);
  const action = useAction();
  const t = useT();
  const { lang } = useLang();
  const current = bot.config?.template_name ?? null;
  const label = (id: string) => {
    const tpl = catalog.data?.find((row) => row.name === id);
    return tpl ? templateTitle(tpl, lang) : id;
  };
  return (
    <Modal title={t.bots.detail.changeConfig} onClose={onClose}>
      <ErrorBanner error={list.error} onRetry={list.reload} />
      <div className="field">
        <label>{t.bots.detail.template}</label>
        <select className="select" value={name} onChange={(e) => setName(e.target.value)} disabled={!list.data}>
          <option value="">{list.data ? t.bots.templateModal.choose : t.common.loading}</option>
          {list.data?.templates.map((tpl) => (
            <option key={tpl.name} value={tpl.name}>
              {label(tpl.name)}
              {tpl.name === current ? t.bots.templateModal.currentSuffix : ""}
              {tpl.min_vip_level > 0 ? t.bots.templateModal.levelSuffix(tpl.min_vip_level) : ""}
            </option>
          ))}
        </select>
      </div>
      {name && !confirm && (
        <div className="banner">
          <div className="body">{t.bots.templateModal.warning(name)}</div>
        </div>
      )}
      <ErrorBanner error={action.error} onDismiss={action.clear} />
      <div className="actions">
        <button type="button" className="btn ghost" onClick={onClose}>
          {t.common.cancel}
        </button>
        {!confirm ? (
          <button type="button" className="btn primary" disabled={!name} onClick={() => setConfirm(true)}>
            {t.bots.continueLabel}
          </button>
        ) : (
          <button
            type="button"
            className="btn primary"
            disabled={action.busy}
            onClick={() =>
              void action.run(async () => {
                await api.applyTemplate(bot.bot_id, name);
                onDone(t.bots.templateModal.applied(name));
              })
            }
          >
            {t.bots.templateModal.apply(name)}
          </button>
        )}
      </div>
    </Modal>
  );
}

function RiskDialog({ bot, onClose, onDone }: { bot: Detail; onClose: () => void; onDone: (msg: string) => void }) {
  const [long, setLong] = useState(String(bot.config?.risk?.long ?? 0));
  const [short, setShort] = useState(String(bot.config?.risk?.short ?? 0));
  const action = useAction();
  const t = useT();
  const l = Number(long),
    s = Number(short);
  const valid = Number.isFinite(l) && Number.isFinite(s) && l >= 0 && s >= 0;
  return (
    <Modal title={t.bots.detail.riskLevel} onClose={onClose}>
      <div className="hint">{t.bots.riskModal.hint}</div>
      <div className="field">
        <label>{t.bots.long}</label>
        <input className="input tnum" type="number" step="0.05" min="0" value={long} onChange={(e) => setLong(e.target.value)} />
      </div>
      <div className="field">
        <label>{t.bots.short}</label>
        <input className="input tnum" type="number" step="0.05" min="0" value={short} onChange={(e) => setShort(e.target.value)} />
      </div>
      <ErrorBanner error={action.error} onDismiss={action.clear} />
      <div className="actions">
        <button type="button" className="btn ghost" onClick={onClose}>
          {t.common.cancel}
        </button>
        <button
          type="button"
          className="btn primary"
          disabled={!valid || action.busy}
          onClick={() =>
            void action.run(async () => {
              await api.setRisk(bot.bot_id, l, s);
              onDone(t.bots.riskModal.saved(l, s));
            })
          }
        >
          {t.common.save}
        </button>
      </div>
    </Modal>
  );
}

function SidesDialog({ bot, onClose, onDone }: { bot: Detail; onClose: () => void; onDone: (msg: string) => void }) {
  const [long, setLong] = useState(bot.config?.sides.long ?? false);
  const [short, setShort] = useState(bot.config?.sides.short ?? false);
  const action = useAction();
  const t = useT();
  const changed = long !== (bot.config?.sides.long ?? false) || short !== (bot.config?.sides.short ?? false);
  return (
    <Modal title={t.bots.detail.sides} onClose={onClose}>
      <div className="hint">{t.bots.sidesModal.hint}</div>
      <label className="check">
        <input type="checkbox" checked={long} onChange={(e) => setLong(e.target.checked)} /> {t.bots.long}
      </label>
      <label className="check">
        <input type="checkbox" checked={short} onChange={(e) => setShort(e.target.checked)} /> {t.bots.short}
      </label>
      <ErrorBanner error={action.error} onDismiss={action.clear} />
      <div className="actions">
        <button type="button" className="btn ghost" onClick={onClose}>
          {t.common.cancel}
        </button>
        <button
          type="button"
          className="btn primary"
          disabled={!changed || action.busy}
          onClick={() =>
            void action.run(async () => {
              if (long !== (bot.config?.sides.long ?? false)) await api.setSide(bot.bot_id, "long", long);
              if (short !== (bot.config?.sides.short ?? false)) await api.setSide(bot.bot_id, "short", short);
              onDone(t.bots.sidesModal.saved(long, short));
            })
          }
        >
          {t.common.save}
        </button>
      </div>
    </Modal>
  );
}

function RuntimeDialog({ bot, onClose, onDone }: { bot: Detail; onClose: () => void; onDone: (msg: string) => void }) {
  const [rt, setRt] = useState<"py" | "rs">(bot.runtime);
  const action = useAction();
  const t = useT();
  return (
    <Modal title={t.bots.detail.runtime} onClose={onClose}>
      <div className="hint">{t.bots.runtimeModal.hint}</div>
      {(["py", "rs"] as const).map((v) => (
        <label key={v} className="check">
          <input type="radio" name="rt" checked={rt === v} onChange={() => setRt(v)} /> {runtimeLabel(v)}
        </label>
      ))}
      <ErrorBanner error={action.error} onDismiss={action.clear} />
      <div className="actions">
        <button type="button" className="btn ghost" onClick={onClose}>
          {t.common.cancel}
        </button>
        <button
          type="button"
          className="btn primary"
          disabled={rt === bot.runtime || action.busy}
          onClick={() =>
            void action.run(async () => {
              await api.setRuntime(bot.bot_id, rt);
              onDone(t.bots.runtimeModal.saved(runtimeLabel(rt)));
            })
          }
        >
          {t.common.save}
        </button>
      </div>
    </Modal>
  );
}
