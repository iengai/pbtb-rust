import { useCallback, useEffect, useState } from "react";
import { useNavigate, useParams } from "react-router-dom";
import { api, isRetryConflict } from "../api/client";
import { useAction, useLoad } from "../api/hooks";
import type { BotDetail as Detail } from "../api/types";
import { RangeSelector, ReturnChart } from "../chart/ReturnChart";
import {
  type BotReturnSeries,
  type ChartWindow,
  DEFAULT_RANGE,
  RANGES,
  fmtSignedPct,
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
} from "../components/ui";
import { chartIdFor, staticData } from "../data/static";

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
  const [notice, setNotice] = useState<string | null>(null);

  const name = bot.data?.name;
  useEffect(() => {
    if (!name) return;
    let alive = true;
    (async () => {
      const index = await staticData.chartIndex().catch(() => []);
      const cid = chartIdFor(index, name);
      const s = cid ? await staticData.chart(cid).catch(() => null) : null;
      if (alive) setSeries(s);
    })();
    return () => {
      alive = false;
    };
  }, [name]);

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
  const rangeK = RANGES[range]!.k;

  const start = () =>
    action.run(async () => {
      const r = await api.startBot(id);
      done(
        r.status === "started"
          ? "Start requested. The task will report Running shortly."
          : r.status === "already_running"
            ? "The bot is already running."
            : "The bot is already starting.",
      );
    });

  return (
    <>
      <Crumbs items={[{ to: "/bots", label: "Bots" }, { label: d?.name ?? id }]} />
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
            Dismiss
          </button>
        </div>
      )}
      {!d && bot.loading && <Loading what="bot" />}
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
                Desired <b style={{ color: "var(--text)", fontWeight: 550 }}>{d.enabled ? "Enabled" : "Disabled"}</b>
                {" · "}Runtime <b style={{ color: "var(--text)", fontWeight: 550 }}>{runtimeLabel(d.runtime)}</b>
                {d.observed_at ? ` · Task observed ${relativeTime(d.observed_at)}` : " · No task observed yet"}
              </div>
            </div>
            <div className="btn-row">
              <button type="button" className="btn" onClick={() => setDialog("stop")} disabled={action.busy}>
                <Stop />
                Stop bot
              </button>
              <button type="button" className="btn primary" onClick={() => void start()} disabled={action.busy}>
                <Play />
                Run bot
              </button>
            </div>
          </div>

          <div className="tiles">
            <Tile
              k={`Return · ${ok?.stats.label ?? rangeK}`}
              v={ok ? fmtSignedPct(ok.stats.ret) : "—"}
              tone={ok ? (ok.stats.ret >= 0 ? "up" : "down") : undefined}
            />
            <Tile k={`Max drawdown · ${ok?.stats.label ?? rangeK}`} v={ok ? `−${Math.abs(ok.stats.maxDrawdown).toFixed(1)}%` : "—"} />
            <Tile k="Leverage" v={d.config?.leverage != null ? `${d.config.leverage}x` : "—"} />
            <Tile k="Config switches" v={series?.config_switches?.length ?? "—"} />
          </div>

          <div className="card tight" style={{ marginBottom: 18 }}>
            <div className="card-title chart">
              <div>Cumulative return</div>
              <RangeSelector value={range} onChange={setRange} />
            </div>
            {series === undefined && <Loading what="chart" />}
            {series === null && (
              <div className="msg">
                No return data for this bot yet.
                <br />
                <span className="hint">The daily collector publishes a series once the bot has traded.</span>
              </div>
            )}
            {win && <ReturnChart window={win} />}
            <div className="legend">
              <span>
                <i className="swatch" style={{ background: "var(--pnl)" }} /> Cumulative return
              </span>
            </div>
            <div className="hint" style={{ marginTop: 4 }}>
              Orange dot: config switch.{ok?.footer ? ` ${ok.footer}` : ""}
            </div>
          </div>

          <div className="two-col">
            <div className="card">
              <div className="card-title">Configuration</div>
              {d.config ? (
                <div className="kv">
                  <div className="k">Template</div>
                  <div style={{ display: "flex", alignItems: "center", gap: 8, minWidth: 0 }}>
                    <span className="mono ellipsis" style={{ fontSize: 13 }}>
                      {d.config.template_name}
                    </span>
                    {d.config.template_version && <Badge>{engineLabel(d.config.template_version)}</Badge>}
                  </div>
                  <div className="k">Tuned on</div>
                  <div>{d.config.tuned_on ? `${d.config.tuned_on} data` : "—"}</div>
                  <div className="k">Strategy</div>
                  <div>
                    {d.config.strategies.length
                      ? d.config.strategies.map((s) => `${s.name} (${s.side})`).join(", ")
                      : "—"}
                  </div>
                  <div className="k">Sides</div>
                  <div style={{ display: "flex", gap: 8 }}>
                    <Pill tone={d.config.sides.long ? "ok" : undefined}>Long {d.config.sides.long ? "on" : "off"}</Pill>
                    <Pill tone={d.config.sides.short ? "ok" : undefined}>Short {d.config.sides.short ? "on" : "off"}</Pill>
                  </div>
                  <div className="k">Risk level</div>
                  <div className="tnum">
                    {d.config.risk ? `Long ${d.config.risk.long.toFixed(2)} · Short ${d.config.risk.short.toFixed(2)}` : "—"}
                  </div>
                  <div className="k">Coins</div>
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
                  No config yet. Choose one to make this bot runnable.
                </div>
              )}
              <div className="btn-row" style={{ marginTop: 16 }}>
                <button type="button" className="btn" onClick={() => setDialog("template")}>
                  Change config
                </button>
                <button type="button" className="btn" onClick={() => setDialog("risk")} disabled={!d.config}>
                  Risk level
                </button>
                <button type="button" className="btn" onClick={() => setDialog("sides")} disabled={!d.config}>
                  Sides
                </button>
                <button type="button" className="btn" onClick={() => setDialog("runtime")}>
                  Runtime
                </button>
              </div>
              <div className="hint" style={{ marginTop: 10 }}>
                Changes apply on the bot's next start. The running task keeps the config it started with.
              </div>
            </div>
            <div className="stack">
              <div className="card">
                <div className="card-title sm">Balance</div>
                <div className="tnum" style={{ fontSize: 20, fontWeight: 640, color: "var(--muted)" }}>
                  —
                </div>
                <div className="hint" style={{ marginTop: 4 }}>
                  Balance lookup is not available yet.
                </div>
              </div>
              <div className="card">
                <div className="card-title sm">Danger zone</div>
                <div className="btn-col">
                  <button
                    type="button"
                    className="btn"
                    disabled={action.busy}
                    onClick={() => void action.run(() => api.unstuck(id))}
                  >
                    Unstuck
                  </button>
                  <button type="button" className="btn danger" onClick={() => setDialog("delete")}>
                    <Trash />
                    Delete bot and API key
                  </button>
                </div>
                <div className="hint" style={{ marginTop: 10 }}>
                  Deleting asks you to type the bot id. It removes the stored API key and config; it does not
                  touch the exchange account.
                </div>
              </div>
            </div>
          </div>

          {dialog === "stop" && (
            <Modal title="Stop this bot?" onClose={close}>
              <div style={{ fontSize: 14 }}>
                The task stops and the bot is marked Disabled, so it is not restarted automatically. Open
                positions stay on the exchange as they are.
              </div>
              <div className="actions">
                <button type="button" className="btn ghost" onClick={close}>
                  Cancel
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
                          ? "Stop requested."
                          : r.status === "not_running"
                            ? "The bot was not running; it is now Disabled."
                            : "The bot is already stopping.",
                      );
                    })
                  }
                >
                  Stop bot
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
  return (
    <Modal title="Delete bot and API key" onClose={onClose}>
      <div style={{ fontSize: 14 }}>
        This removes <b>{bot.name}</b>, its config and its stored exchange keys. It cannot be undone. Type the
        bot id to confirm:
      </div>
      <div className="mono" style={{ fontSize: 13, color: "var(--muted)", userSelect: "all" }}>
        {bot.bot_id}
      </div>
      <input
        className="input mono"
        value={typed}
        onChange={(e) => setTyped(e.target.value)}
        placeholder="bot id"
        autoFocus
      />
      <ErrorBanner error={action.error} onDismiss={action.clear} />
      <div className="actions">
        <button type="button" className="btn ghost" onClick={onClose}>
          Cancel
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
          Delete
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
  const [name, setName] = useState("");
  const [confirm, setConfirm] = useState(false);
  const action = useAction();
  const current = bot.config?.template_name ?? null;
  return (
    <Modal title="Change config" onClose={onClose}>
      <ErrorBanner error={list.error} onRetry={list.reload} />
      <div className="field">
        <label>Template</label>
        <select className="select" value={name} onChange={(e) => setName(e.target.value)} disabled={!list.data}>
          <option value="">{list.data ? "Choose a template…" : "Loading…"}</option>
          {list.data?.templates.map((t) => (
            <option key={t} value={t}>
              {t}
              {t === current ? " (current)" : ""}
            </option>
          ))}
        </select>
      </div>
      {name && !confirm && (
        <div className="banner">
          <div className="body">
            Switching to <span className="mono">{name}</span> replaces the bot's strategy, sides, coins and
            risk settings with the template's. It applies on the next start.
          </div>
        </div>
      )}
      <ErrorBanner error={action.error} onDismiss={action.clear} />
      <div className="actions">
        <button type="button" className="btn ghost" onClick={onClose}>
          Cancel
        </button>
        {!confirm ? (
          <button type="button" className="btn primary" disabled={!name} onClick={() => setConfirm(true)}>
            Continue
          </button>
        ) : (
          <button
            type="button"
            className="btn primary"
            disabled={action.busy}
            onClick={() =>
              void action.run(async () => {
                await api.applyTemplate(bot.bot_id, name);
                onDone(`Config ${name} applied; it takes effect on the next start.`);
              })
            }
          >
            Apply {name}
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
  const l = Number(long),
    s = Number(short);
  const valid = Number.isFinite(l) && Number.isFinite(s) && l >= 0 && s >= 0;
  return (
    <Modal title="Risk level" onClose={onClose}>
      <div className="hint">
        Wallet exposure limit per side. Leverage is derived as max(long, short) + 1. Applies on the next
        start.
      </div>
      <div className="field">
        <label>Long</label>
        <input className="input tnum" type="number" step="0.05" min="0" value={long} onChange={(e) => setLong(e.target.value)} />
      </div>
      <div className="field">
        <label>Short</label>
        <input className="input tnum" type="number" step="0.05" min="0" value={short} onChange={(e) => setShort(e.target.value)} />
      </div>
      <ErrorBanner error={action.error} onDismiss={action.clear} />
      <div className="actions">
        <button type="button" className="btn ghost" onClick={onClose}>
          Cancel
        </button>
        <button
          type="button"
          className="btn primary"
          disabled={!valid || action.busy}
          onClick={() =>
            void action.run(async () => {
              await api.setRisk(bot.bot_id, l, s);
              onDone(`Risk level set to long ${l.toFixed(2)} · short ${s.toFixed(2)}.`);
            })
          }
        >
          Save
        </button>
      </div>
    </Modal>
  );
}

function SidesDialog({ bot, onClose, onDone }: { bot: Detail; onClose: () => void; onDone: (msg: string) => void }) {
  const [long, setLong] = useState(bot.config?.sides.long ?? false);
  const [short, setShort] = useState(bot.config?.sides.short ?? false);
  const action = useAction();
  const changed = long !== (bot.config?.sides.long ?? false) || short !== (bot.config?.sides.short ?? false);
  return (
    <Modal title="Sides" onClose={onClose}>
      <div className="hint">Enable or disable one side of the strategy. Applies on the next start.</div>
      <label className="check">
        <input type="checkbox" checked={long} onChange={(e) => setLong(e.target.checked)} /> Long
      </label>
      <label className="check">
        <input type="checkbox" checked={short} onChange={(e) => setShort(e.target.checked)} /> Short
      </label>
      <ErrorBanner error={action.error} onDismiss={action.clear} />
      <div className="actions">
        <button type="button" className="btn ghost" onClick={onClose}>
          Cancel
        </button>
        <button
          type="button"
          className="btn primary"
          disabled={!changed || action.busy}
          onClick={() =>
            void action.run(async () => {
              if (long !== (bot.config?.sides.long ?? false)) await api.setSide(bot.bot_id, "long", long);
              if (short !== (bot.config?.sides.short ?? false)) await api.setSide(bot.bot_id, "short", short);
              onDone(`Sides set: long ${long ? "on" : "off"} · short ${short ? "on" : "off"}.`);
            })
          }
        >
          Save
        </button>
      </div>
    </Modal>
  );
}

function RuntimeDialog({ bot, onClose, onDone }: { bot: Detail; onClose: () => void; onDone: (msg: string) => void }) {
  const [rt, setRt] = useState<"py" | "rs">(bot.runtime);
  const action = useAction();
  return (
    <Modal title="Runtime" onClose={onClose}>
      <div className="hint">
        Which image the bot launches on within its engine line. A running task keeps the binary it started
        with.
      </div>
      {(["py", "rs"] as const).map((v) => (
        <label key={v} className="check">
          <input type="radio" name="rt" checked={rt === v} onChange={() => setRt(v)} /> {runtimeLabel(v)}
        </label>
      ))}
      <ErrorBanner error={action.error} onDismiss={action.clear} />
      <div className="actions">
        <button type="button" className="btn ghost" onClick={onClose}>
          Cancel
        </button>
        <button
          type="button"
          className="btn primary"
          disabled={rt === bot.runtime || action.busy}
          onClick={() =>
            void action.run(async () => {
              await api.setRuntime(bot.bot_id, rt);
              onDone(`Runtime set to ${runtimeLabel(rt)}.`);
            })
          }
        >
          Save
        </button>
      </div>
    </Modal>
  );
}
