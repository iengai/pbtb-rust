// The public showcase: the operator's bots without a sign-in, and the live
// runs a config page shows under its backtest.
export const showcase = {
  title: "Showcase",
  lead: "Live return curves of the operator's own bots, collected daily from Bybit. Percentages only; the capital behind a config is shown rounded.",
  disclaimer: "Live results of real accounts. Past performance is not future returns.",
  nothing: "Nothing public yet.",
  notFound: "No such public bot.",
  loading: "showcase",
  loadingBot: "bot",
  onBybit: "View on Bybit",
  col: { bot: "Bot", trend: "Last 30 days", current: "Return" },
  currentReturn: "Current return",
  updated: (date: string) => `updated ${date} UTC`,
  bands: "Orange dot: config switch. The band behind the curve is the config's active period; hover to highlight it, click its name to open the config.",
  runs: {
    title: "Live runs",
    lead: (min: number, max: number) =>
      `Every span of at least ${min} days a showcase bot ran this config, re-based to its start; the ${max} most recent when there are more.`,
    ongoing: "ongoing",
    caption: (c: { cap: string; start: string; end: string; days: number; ret: string }) =>
      `capital ${c.cap} · ${c.start} → ${c.end} · ${c.days} days · ${c.ret}`,
  },
};
