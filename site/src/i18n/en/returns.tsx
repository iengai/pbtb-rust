// The return-curve page, plus every word the return chart shows: captions,
// empty states, tooltip rows and axis ticks.
export const returns = {
  title: "Return Curves",
  lead: "Time-weighted return and realized PnL of your bots, collected daily from Bybit. Balances stay off the page.",
  noBots: "You have no bots yet.",
  noData: "No return data for this bot yet — the daily collector publishes a series once it has traded.",
  botLabel: "Bot",
  loadingBots: "bots",
  loadingCurve: "return curve",
  tile: {
    return: (range: string) => `${range} return`,
    peak: "Peak",
    days: "Days",
    pnl: (range: string) => `${range} net PnL`,
    totalPnl: "Net PnL, all time",
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
    pnlRow: "Day's net PnL",
    configRow: "Config",
    switchTitle: (template: string, date: string) => `→ ${template} · ${date}`,
    periodTitle: (template: string, from: string, to: string) => `${template} · ${from} → ${to}`,
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
};
