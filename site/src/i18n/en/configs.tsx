import type { ReactNode } from "react";

// The Configs list, a template's detail page and the metric labels the backtest
// table looks up by passivbot metric key.
export const configs = {
  // A backtest the engine cut short is an account that was liquidated inside the
  // window, so both pages mark it.
  liquidatedBadge: "liquidated",
  wipedOut: "wiped out",
  // A template's naming properties, shown as tags: the strategy family, keyed
  // by `pbtb.style`, and the lab iteration that produced the tuning.
  style: { grid: "Grid", martingale: "Martingale", ema_anchor: "EMA anchor" },
  generation: (n: number) => `Gen ${n}`,

  list: {
    title: "Configs",
    lead: (shown: number, total: number, exchanges: string) =>
      `${shown} of ${total} strategy templates shown · backtested on ${exchanges || "exchange"} data · sorted by gain`,
    allEngines: "All",
    templates: "templates",
    empty: "No templates published yet.",
    maxDd: "Max DD",
    backtestRange: (start: string, end: string, exchange: string) => `Backtest ${start} → ${end} · ${exchange}`,
  },

  detail: {
    template: "template",
    lead: (exchange: string, start: string, end: string, coins: number) =>
      `Tuned on ${exchange} data · backtest ${start} → ${end} · ${coins} coin${coins === 1 ? "" : "s"}`,
    liquidatedNote:
      "the account was liquidated before the window ended; the metrics describe the run up to that point",
    applyCta: "Apply to a bot…",
    aboutTitle: "About this strategy",
    noDescription: "No description.",
    setupTitle: "Setup",
    sides: "Sides",
    coins: "Coins",
    style: "Style",
    generation: "Generation",
    engine: "Engine",
    engineValue: (version: string) => `passivbot ${version} · runs on py or rs`,
    metricsTitle: "Backtest metrics",
    metricsHint: "USD figures from analysis.json",
  },

  // The chart card: the backtest and the live runs on one chart.
  chart: {
    title: "Backtest and live runs",
    aria: "backtest and live returns",
    brushAria: "period",
    hint: "Every curve starts at 0% at its first point inside the period. Pick a preset, or drag on the strip under the chart for any span.",
    backtest: "Backtest equity",
    balance: "Backtest balance",
    notEnough: "Not enough backtest points to plot.",
  },

  apply: {
    title: (template: string) => `Apply ${template}`,
    botLabel: "Bot",
    chooseBot: "Choose a bot…",
    warning: (bot: string): ReactNode => (
      <>
        This replaces <b>{bot}</b>&apos;s strategy, sides, coins and risk settings with the template&apos;s. It
        applies on the bot&apos;s next start.
      </>
    ),
    proceed: "Continue",
    applyTo: (bot: string) => `Apply to ${bot}`,
    done: (template: string, bot: string) => `Applied ${template} to ${bot}; it takes effect on the next start.`,
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
  },
};
