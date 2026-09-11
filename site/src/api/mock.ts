// A dev-only fake of `/api/v1` and of a signed-in session, so the pages can be
// worked on without an issuer or a Lambda. Loaded only when
// `VITE_MOCK_API=1` under `vite dev`; never part of a production bundle.

import { saveSession } from "../auth/oauth";
import { api, ApiError } from "./client";
import type { BotDetail, BotSummary, ConfigDescription, Me, Phase } from "./types";
import type { BotReturnSeries } from "../chart/returnCurve";

const now = () => Math.floor(Date.now() / 1000);

const config = (name: string, version: string, coins: string[], short = false): ConfigDescription => ({
  template_name: name,
  title: null,
  title_zh: null,
  template_version: version,
  description: null,
  tuned_on: "bybit",
  config_version: version.startsWith("8") ? 8 : 7,
  strategies: [{ name: name.replace(/-v\d+$/, ""), side: short ? "long,short" : "long" }],
  sides: { long: true, short },
  risk: { long: 1.75, short: short ? 0.5 : 0 },
  leverage: 2.75,
  coins: { long: coins, short: short ? coins.slice(0, 2) : [] },
  updated_at: now() - 86400 * 3,
});

// A deterministic random walk per bot, so the charts have something to draw
// and the same bot draws the same curve on every reload.
function fakeSeries(b: BotDetail): BotReturnSeries {
  let seed = [...b.bot_id].reduce((h, c) => (h * 31 + c.charCodeAt(0)) >>> 0, 7);
  const rand = () => ((seed = (seed * 1664525 + 1013904223) >>> 0) / 2 ** 32) * 2 - 1;
  const days = 120;
  const start = now() - 86400 * days;
  const points: BotReturnSeries["points"] = [];
  let index = 100;
  let cum = 0;
  const stake = 1000;
  for (let i = 0; i <= days; i++) {
    const dr = rand() * 0.02 + 0.0015;
    index *= 1 + dr;
    const realized = i === 0 ? 0 : stake * dr;
    cum += realized;
    points.push({
      ts: start + 86400 * i,
      index,
      return_pct: (index / 100 - 1) * 100,
      realized_usdt: realized,
      cum_realized_usdt: cum,
    });
  }
  return {
    id: b.bot_id,
    name: b.name,
    exchange: b.exchange,
    generated_at: now(),
    current_return_pct: points[points.length - 1]!.return_pct,
    total_realized_usdt: cum,
    points,
    config_switches: b.config ? [{ ts: b.config.updated_at, template_name: b.config.template_name }] : [],
    capital_resets: [],
  };
}

let bots: BotDetail[] = [
  {
    bot_id: "b-dollardigger",
    name: "DollarDigger",
    exchange: "bybit",
    enabled: true,
    runtime: "py",
    phase: "running",
    created_at: now() - 86400 * 120,
    updated_at: now() - 86400 * 3,
    task_id: "arn:aws:ecs:ap-northeast-1:000000000000:task/x/abc",
    observed_at: now() - 86400 * 3,
    restarts: 4,
    config: config("bybit-cap300-iter1-winner-v712", "7.12.0", ["SOL", "XRP", "DOGE", "ADA", "LINK", "AVAX", "SUI", "TON"]),
  },
  {
    bot_id: "b-lowrisk",
    name: "Low-Risk Trader",
    exchange: "bybit",
    enabled: true,
    runtime: "py",
    phase: "running",
    created_at: now() - 86400 * 200,
    updated_at: now() - 86400 * 10,
    task_id: "arn:task/def",
    observed_at: now() - 3600 * 5,
    restarts: 1,
    config: config("bybit-cap1000-iter4-winner-v712", "7.12.0", ["SOL", "XRP", "DOGE", "ADA", "LINK"]),
  },
  {
    bot_id: "b-abot",
    name: "abot",
    exchange: "bybit",
    enabled: true,
    runtime: "rs",
    phase: "starting",
    created_at: now() - 86400 * 30,
    updated_at: now() - 600,
    task_id: null,
    observed_at: now() - 60,
    restarts: 0,
    config: config("bybit-cap100-iter1-winner-v810", "8.1.0", ["SOL", "XRP", "DOGE"], true),
  },
  {
    bot_id: "b-paper",
    name: "PaperTrader",
    exchange: "bybit",
    enabled: false,
    runtime: "py",
    phase: "stopped",
    created_at: now() - 86400 * 400,
    updated_at: now() - 86400 * 40,
    task_id: null,
    observed_at: now() - 86400 * 40,
    restarts: 9,
    config: null,
  },
];

const me: Me = {
  user_id: "5351234539",
  vip_level: 9,
  scopes: ["bots:read", "bots:write"],
  telegram: "5351234539",
  identities: [
    { provider: "workos", subject: "user_01MOCKSUBJECT" },
    { provider: "telegram", subject: "5351234539" },
  ],
};

const summary = (b: BotDetail): BotSummary => ({
  bot_id: b.bot_id,
  name: b.name,
  exchange: b.exchange,
  enabled: b.enabled,
  runtime: b.runtime,
  phase: b.phase,
  created_at: b.created_at,
  updated_at: b.updated_at,
});

const find = (id: string) => {
  const b = bots.find((x) => x.bot_id === id);
  if (!b) throw new ApiError(404, { error: "not found" }, "not found");
  return b;
};
const delay = <T,>(v: T, ms = 250) => new Promise<T>((r) => setTimeout(() => r(v), ms));
const settle = (b: BotDetail, phase: Phase, ms: number) =>
  setTimeout(() => {
    b.phase = phase;
    b.observed_at = now();
  }, ms);

export function installMock(): void {
  saveSession({
    access_token: "mock",
    expires_at: now() + 3600,
    claims: { sub: "user_01MOCKSUBJECT", email: "you@example.com", scope: "bots:read bots:write" },
  });

  Object.assign(api, {
    me: () => delay(me),
    signup: () => delay({ status: "existing", user_id: me.user_id, vip_level: me.vip_level }),
    bindTicket: () =>
      delay({ token: "a".repeat(64), url: `https://t.me/pbtb_mock_bot?start=${"a".repeat(64)}`, expires_in: 600 }),
    unbindTelegram: () => {
      me.telegram = null;
      me.identities = me.identities.filter((it) => it.provider !== "telegram");
      return delay({ released: 1 });
    },
    listBots: () => delay({ bots: bots.map(summary) }),
    getBot: (id: string) => delay({ ...find(id) }),
    addBot: (body: { name: string; overwrite?: boolean }) => {
      const existing = bots.find((b) => b.name === body.name);
      if (existing && !body.overwrite) {
        return Promise.reject(
          new ApiError(409, { status: "already_exists", bot: summary(existing) }, "already exists"),
        );
      }
      if (existing) return delay({ status: "overwritten", bot: summary(existing) });
      const b: BotDetail = {
        bot_id: `b-${body.name.toLowerCase().replace(/\W+/g, "-")}`,
        name: body.name,
        exchange: "bybit",
        enabled: false,
        runtime: "py",
        phase: null,
        created_at: now(),
        updated_at: now(),
        task_id: null,
        observed_at: null,
        restarts: null,
        config: null,
      };
      bots.push(b);
      return delay({ status: "added", bot: summary(b) });
    },
    deleteBot: (id: string) => {
      find(id);
      bots = bots.filter((b) => b.bot_id !== id);
      return delay({ status: "deleted" });
    },
    startBot: (id: string) => {
      const b = find(id);
      if (b.phase === "running") return delay({ status: "already_running" });
      if (b.phase === "stopping") return Promise.reject(new ApiError(409, { status: "stopping", retry: true }, ""));
      b.enabled = true;
      b.phase = "starting";
      settle(b, "running", 20_000);
      return delay({ status: "started", task_id: "arn:task/new" });
    },
    stopBot: (id: string) => {
      const b = find(id);
      b.enabled = false;
      if (b.phase === "stopped" || b.phase === null) return delay({ status: "not_running" });
      b.phase = "stopping";
      settle(b, "stopped", 20_000);
      return delay({ status: "stopped", task_id: b.task_id });
    },
    setRisk: (id: string, long: number, short: number) => {
      const b = find(id);
      if (long > 3 || short > 3) return Promise.reject(new ApiError(400, { error: "risk level out of range: max 3.0" }, ""));
      if (b.config) b.config.risk = { long, short };
      return delay({ status: "updated" });
    },
    setSide: (id: string, side: "long" | "short", enabled: boolean) => {
      const b = find(id);
      if (b.config) b.config.sides[side] = enabled;
      return delay({ status: "updated" });
    },
    setRuntime: (id: string, runtime: "py" | "rs") => {
      const b = find(id);
      b.runtime = runtime;
      return delay({ status: "updated" });
    },
    applyTemplate: (id: string, name: string) => {
      const b = find(id);
      b.config = config(name, name.endsWith("v810") ? "8.1.0" : "7.12.0", ["SOL", "XRP", "DOGE"]);
      return delay({ status: "applied" });
    },
    unstuck: () =>
      Promise.reject(new ApiError(501, { error: "not available yet" }, "not available yet")),
    botReturns: (id: string) => {
      const b = find(id);
      if (b.phase === null) return Promise.reject(new ApiError(404, { error: "not found" }, "not found"));
      return delay(fakeSeries(b));
    },
    listTemplates: () =>
      delay({
        templates: [
          { name: "bybit-cap1000-iter7-winner-v810", min_vip_level: 0 },
          { name: "bybit-cap300-iter1-winner-v712", min_vip_level: 0 },
          { name: "bybit-cap100-iter1-winner-v810", min_vip_level: 3 },
        ],
      }),
    getTemplate: (name: string) => delay({ name, version: "8.1.0", description: null, min_vip_level: 0 }),
  });
}
