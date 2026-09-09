// The return-curve page, plus every word the two SVG charts show: captions,
// empty states, tooltip rows and axis ticks.
export const returns = {
  title: "Return Curves",
  lead: "Time-weighted return of your bots, collected daily from Bybit. Normalized — no account size shown.",
  noBots: "You have no bots yet.",
  noData: "No return data for this bot yet — the daily collector publishes a series once it has traded.",
  botLabel: "Bot",
  loadingBots: "bots",
  loadingCurve: "return curve",
  tile: {
    return: (range: string) => `${range} return`,
    peak: "Peak",
    days: "Days",
  },
  // The window a curve covers, named for what it measures.
  range: {
    sinceRefunding: "Since re-funding",
    total: "Total",
  },
  legend: {
    cumulative: "Cumulative return",
    configSwitch: "Config switch",
  },
  chart: {
    aria: "return curve",
    returnRow: "Return",
    switchTitle: (template: string, date: string) => `→ ${template} · ${date}`,
    axisDate: (month: string, day: number) => `${month} ${day}`,
    caption: (c: { exchange: string; days: number; resetAt: string | null; updatedAt: string }) =>
      [
        c.exchange,
        `${c.days} ${c.days === 1 ? "day" : "days"} shown`,
        "time-weighted, deposit-adjusted",
        ...(c.resetAt ? [`index restarted ${c.resetAt}, when the wiped account was re-funded`] : []),
        `updated ${c.updatedAt} UTC`,
      ].join(" · "),
    empty: {
      noData: "Not enough data to plot yet.",
      refunded: (date: string) =>
        `The account was re-funded on ${date} — not enough data since then to plot.`,
      refundedHint: "Everything before that date was earned on capital that no longer exists.",
      wipedOut: (range: string) =>
        `Account was already at zero when this ${range} window opened — no return to compute.`,
      wipedOutHint: "The capital was lost earlier in this bot's history.",
    },
  },
  // The backtest equity chart, shown on a template's page.
  equity: {
    aria: "backtest equity",
    notEnough: "Not enough backtest points to plot.",
    equity: "Equity",
    balance: "Balance",
  },
};
