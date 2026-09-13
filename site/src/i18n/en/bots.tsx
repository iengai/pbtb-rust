import type { ReactNode } from "react";

// The bots area: the listing, one bot's detail with all of its dialogs, and the
// three-step add flow. Bot names, ids, template names, coin symbols, exchange
// ids and runtime labels are data, not copy, so they are not in here.
export const bots = {
  enabled: "Enabled",
  disabled: "Disabled",
  long: "Long",
  short: "Short",
  continueLabel: "Continue",

  // --- Bots: the listing ---
  list: {
    title: "Bots",
    summary: (total: number, running: number, starting: number) =>
      `${total} bot${total === 1 ? "" : "s"} · ${running} running` +
      (starting ? ` · ${starting} starting` : "") +
      " · time-weighted return, normalized",
    loadingWhat: "bots",
    addBot: "Add bot",
    th: {
      bot: "Bot",
      trend: "Trend",
      ret: "Return",
      actual: "Actual",
      desired: "Desired",
      config: "Config",
      runtime: "Runtime",
    },
    empty: "No bots yet. Add one to get started.",
    note:
      "Desired is what you asked for. Actual is the task ECS last reported. A bot that is Enabled but " +
      "Stopped comes back by itself after an out-of-memory stop or a Restart; after anything else, Run it.",
  },

  // --- BotDetail: header, chart, configuration and danger zone ---
  detail: {
    loadingWhat: "bot",
    loadingChart: "chart",
    startRequested: "Start requested. The task will report Running shortly.",
    alreadyRunning: "The bot is already running.",
    alreadyStarting: "The bot is already starting.",
    desiredRuntime: (enabled: ReactNode, runtime: ReactNode) => (
      <>
        Desired {enabled}
        {" · "}Runtime {runtime}
      </>
    ),
    taskObserved: (when: string) => `Task observed ${when}`,
    noTaskObserved: "No task observed yet",
    stopBot: "Stop bot",
    restartBot: "Restart",
    runBot: "Run bot",
    returnTile: (window: string) => `Return · ${window}`,
    maxDrawdownTile: (window: string) => `Max drawdown · ${window}`,
    netPnlTile: (window: string) => `Net PnL · ${window}`,
    leverage: "Leverage",
    configSwitches: "Config switches",
    cumulativeReturn: "Cumulative return",
    noReturnData: "No return data for this bot yet.",
    noReturnDataHint: "The daily collector publishes a series once the bot has traded.",
    switchDot:
      "Orange dot: config switch. The band behind the curve is that config's active period; hover to highlight it, click its name to open the config.",
    configuration: "Configuration",
    template: "Template",
    tunedOn: "Tuned on",
    tunedOnValue: (source: string) => `${source} data`,
    strategy: "Strategy",
    strategyEntry: (name: string, side: string) => `${name} (${side})`,
    sides: "Sides",
    sidePillLong: (on: boolean) => `Long ${on ? "on" : "off"}`,
    sidePillShort: (on: boolean) => `Short ${on ? "on" : "off"}`,
    riskLevel: "Risk level",
    riskValue: (long: number, short: number) => `Long ${long.toFixed(2)} · Short ${short.toFixed(2)}`,
    coins: "Coins",
    noConfig: "No config yet. Choose one to make this bot runnable.",
    changeConfig: "Change config",
    runtime: "Runtime",
    configHint:
      "Changes apply on the bot's next start or Restart. The running task keeps the config it started with.",
    balance: "Balance",
    balanceHint: "Balance lookup is not available yet.",
    dangerZone: "Danger zone",
    unstuck: "Unstuck",
    deleteBot: "Delete bot and API key",
    deleteHint:
      "Deleting asks you to type the bot id. It removes the stored API key and config; it does not touch " +
      "the exchange account.",
  },

  // --- BotDetail: stop confirmation ---
  stopModal: {
    title: "Stop this bot?",
    body:
      "The task stops and the bot is marked Disabled, so it is not restarted automatically. Open positions " +
      "stay on the exchange as they are.",
    stopped: "Stop requested.",
    notRunning: "The bot was not running; it is now Disabled.",
    alreadyStopping: "The bot is already stopping.",
  },

  // --- BotDetail: restart confirmation ---
  restartModal: {
    title: "Restart this bot?",
    body:
      "The task stops and comes back with the current config; the bot stays Enabled. Open positions stay " +
      "on the exchange as they are.",
    restarting: "Restart requested. The task stops, then reports Starting and Running.",
    started: "The bot was not running; it is starting now.",
  },

  // --- BotDetail: delete confirmation, typed against the bot id ---
  deleteModal: {
    body: (name: ReactNode) => (
      <>
        This removes <b>{name}</b>, its config and its stored exchange keys. It cannot be undone. Type the
        bot id to confirm:
      </>
    ),
    placeholder: "bot id",
  },

  // --- BotDetail: apply a template ---
  templateModal: {
    choose: "Choose a template…",
    currentSuffix: " (current)",
    levelSuffix: (level: number) => ` · VIP ${level}+`,
    warning: (name: ReactNode) => (
      <>
        Switching to <span className="mono">{name}</span> replaces the bot's strategy, sides, coins and risk
        settings with the template's. It applies on the next start or Restart.
      </>
    ),
    apply: (name: string) => `Apply ${name}`,
    applied: (name: string) => `Config ${name} applied; it takes effect on the next start or Restart.`,
  },

  // --- BotDetail: wallet exposure per side ---
  riskModal: {
    hint:
      "Wallet exposure limit per side. Leverage is derived as max(long, short) + 1. Applies on the next " +
      "start or Restart.",
    saved: (long: number, short: number) =>
      `Risk level set to long ${long.toFixed(2)} · short ${short.toFixed(2)}.`,
  },

  // --- BotDetail: enable or disable one side ---
  sidesModal: {
    hint: "Enable or disable one side of the strategy. Applies on the next start or Restart.",
    saved: (long: boolean, short: boolean) =>
      `Sides set: long ${long ? "on" : "off"} · short ${short ? "on" : "off"}.`,
  },

  // --- BotDetail: which image the bot launches on ---
  runtimeModal: {
    hint:
      "Which image the bot launches on within its engine line. A running task keeps the binary it started " +
      "with.",
    saved: (runtime: string) => `Runtime set to ${runtime}.`,
  },

  // --- AddBot: name, key, secret ---
  add: {
    title: "Add bot",
    lead: "One bot per exchange sub-account. The key is stored encrypted and never shown again.",
    steps: {
      name: "Name",
      apiKey: "API key",
      secret: "Secret",
    },
    nameLabel: "Bot name",
    keyLabel: "Bybit API key",
    keyPlaceholder: "Paste the API key",
    keyHint: (ip: ReactNode) => <>Read + trade permissions. IP whitelist: {ip}.</>,
    egressFallback: "the egress address shown on the Account page",
    secretLabel: "API secret",
    secretPlaceholder: "Paste the API secret",
    secretHint: "Sent once over TLS to the API, stored encrypted, never echoed back.",
    replaceKey: "Replace the stored key",
    conflict: (name: string) => (
      <>
        <b>A bot named “{name}” already exists.</b> Continuing will replace its stored API key. Its config
        and history stay.
      </>
    ),
  },
};
