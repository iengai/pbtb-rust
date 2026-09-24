import type { ReactNode } from "react";

// The Configs list, a template's detail page and the metric labels the backtest
// table looks up by passivbot metric key.
export const configs = {
  // A backtest the engine cut short is an account that was liquidated inside the
  // window, so both pages mark it.
  liquidatedBadge: "liquidated",
  wipedOut: "wiped out",
  // A retired template: offered to the operator's account only.
  retiredBadge: "retired",
  // How many coins a template holds at once, as a class: the list's first split
  // and the first tag on a card.
  positions: { single: "Single position", multi: "Multi position" },
  // The strategy's order logic, keyed by `pbtb.style`. One family,
  // martingale-style averaging: `grid` is passivbot v7's trailing grid, which v8
  // keeps as a deprecated compatibility strategy beside the trailing martingale.
  // Two surfaces outside the site word these the same and are edited with this
  // line: `style_label` in src/interface/telegram/views.rs, and STYLES_ZH in
  // scripts/describe_templates.py (which words the member-facing description).
  style: { grid: "Trailing grid (deprecated v7 form)", martingale: "Trailing martingale", ema_anchor: "EMA anchor" },
  generation: (n: number) => `Gen ${n}`,

  list: {
    title: "Configs",
    lead: (shown: number, total: number, exchanges: string) =>
      `${shown} of ${total} strategy templates shown · backtested on ${exchanges || "exchange"} data`,
    // The operator's two catalogues; everyone else sees the published one alone.
    tabs: { published: "Published", retired: "Retired" },
    allPositions: "All",
    allExchanges: "All exchanges",
    templates: "templates",
    empty: { published: "No templates published yet.", retired: "No retired templates." },
    maxDd: "Max DD",
    // Under a card's drawdown: the worst the same parameters drew down at any balance of the capital profile.
    worstDd: (dd: string) => `worst by capital ${dd}`,
    sortBy: "Sort",
    sorts: { capital: "Min capital", gain: "Gain", drawdown: "Max DD", sharpe: "Sharpe" },
    // The bots that hold the template now: the account's own, and the showcase's.
    myBots: "My bots",
    showcaseBots: "Showcase",
    backtestRange: (start: string, end: string, exchange: string) => `Backtest ${start} → ${end} · ${exchange}`,
  },

  detail: {
    template: "template",
    lead: (exchange: string, start: string, end: string, coins: number) =>
      `Backtested on ${exchange} data · ${start} → ${end} · ${coins} coin${coins === 1 ? "" : "s"}`,
    // A backtest stepped by a candle wider than a minute: Hyperliquid's, which serves no
    // 1-minute history older than its latest 5000 candles.
    candleNote: (hours: number) =>
      `Run on ${hours}-hour candles: the exchange serves no older 1-minute history. A drawdown read at ${hours} hour${hours === 1 ? "" : "s"} runs higher than at 1 minute.`,
    copyNote: "The description quotes the same parameters' multi-year 1-minute backtest on Bybit.",
    liquidatedNote:
      "the account was liquidated before the window ended; the metrics describe the run up to that point",
    retiredNote: "retired: offered to the operator's account only",
    applyCta: "Apply to a bot…",
    aboutTitle: "About this strategy",
    noDescription: "No description.",
    setupTitle: "Setup",
    sides: "Sides",
    coins: "Coins",
    positions: "Positions",
    positionsHint: {
      single: "Holds one coin at a time: the whole exposure limit sits in one position.",
      multi: "Holds several coins at once, each with a share of the exposure limit.",
    },
    style: "Order logic",
    generation: "Generation",
    engine: "Engine",
    engineValue: (version: string) => `passivbot ${version} · runs on py or rs`,
    // The capital in a template's title: the least it is offered for, which the
    // backtest started with.
    minCapital: "Min capital",
    minCapitalHint: "The backtest starts here. An account with more runs the same parameters.",
    // Appended when the page shows a capital profile.
    minCapitalProfileHint: "That is not the same result: see the capital profile below.",
    metricsTitle: "Backtest metrics",
    metricsHint: "USD figures from analysis.json",
  },

  // The capital profile: the same parameters and window, started from each balance.
  profile: {
    title: "Capital profile",
    hint:
      "The same parameters over the same window, started from each balance. The min capital is the least the template is offered for, not a promise that more behaves the same: a small balance cannot place the first order on an expensive coin, and a template that holds one position can take a different path at one balance. Read the worst row as the risk.",
    balance: "Capital",
    traded: "Coins traded (share of fills)",
  },

  // The chart card: the backtest and the live runs on one chart.
  chart: {
    title: "Backtest and live runs",
    aria: "backtest and live returns",
    brushAria: "period",
    hint: "Every curve starts at 0% at its first point inside the period. Pick a preset, or drag on the strip under the chart for any span. The log axis spaces equal multiples equally.",
    scale: { log: "Log", linear: "Linear" },
    backtest: "Backtest equity",
    balance: "Backtest balance",
    notEnough: "Not enough backtest points to plot.",
  },

  // The operator's switch between the two catalogues.
  audience: {
    retire: "Retire",
    publish: "Publish",
    title: {
      retire: (template: string) => `Retire ${template}`,
      publish: (template: string) => `Publish ${template}`,
    },
    body: {
      retire:
        "Members stop being offered it and can no longer apply it; it moves to the Retired tab. Bots already running it keep their config.",
      publish: "Every account is offered it again; it moves to the Published tab.",
    },
    publicNote: "The public catalogue follows within about half a minute.",
    done: {
      retire: (template: string) => `Retired ${template}.`,
      publish: (template: string) => `Published ${template}.`,
    },
  },

  apply: {
    title: (template: string) => `Apply ${template}`,
    botLabel: "Bot",
    chooseBot: "Choose a bot…",
    warning: (bot: string): ReactNode => (
      <>
        This replaces <b>{bot}</b>&apos;s strategy, sides, coins and risk settings with the template&apos;s. It
        applies on the bot&apos;s next start or Restart.
      </>
    ),
    proceed: "Continue",
    applyTo: (bot: string) => `Apply to ${bot}`,
    done: (template: string, bot: string) =>
      `Applied ${template} to ${bot}; it takes effect on the next start or Restart.`,
  },

  metric: {
    gain: "Gain",
    adg: "ADG",
    adg_w: "ADG (weighted)",
    drawdown_worst: "Max drawdown",
    sharpe_ratio: "Sharpe",
    sortino_ratio: "Sortino",
    calmar_ratio: "Calmar",
    positions_held_per_day: "Positions / day",
    position_held_hours_mean: "Position held (h, mean)",
    loss_profit_ratio: "Loss / profit",
    backtest_completion_ratio: "Window completed",
    drawdown_worst_mean_1pct: "Drawdown (worst 1% mean)",
    omega_ratio: "Omega",
    sterling_ratio: "Sterling",
    position_unchanged_hours_max: "Position unchanged (h, max)",
    equity_balance_diff_neg_max: "Unrealized loss (max)",
    exposure_ratios_mean_long: "Exposure (long, mean)",
    exposure_ratios_mean_short: "Exposure (short, mean)",
  },
};
