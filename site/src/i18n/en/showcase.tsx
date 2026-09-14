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
  col: { bot: "Bot", trend: (days: number) => `Last ${days} days` },
  updated: (date: string) => `updated ${date} UTC`,
  // The operator's switchboard: which of their own bots the public page shows.
  manage: {
    cta: "Manage showcase",
    title: "Manage showcase",
    lead: "Your own bots. Choose which ones the public showcase shows; hiding a bot keeps its Bybit link.",
    operatorOnly: "The showcase is managed from the operator's account.",
    loading: "bots",
    col: { bot: "Bot", link: "Bybit link", state: "Showcase" },
    shown: "Public",
    hidden: "Hidden",
    noLink: "no link",
    show: "Show",
    hide: "Hide",
    empty: "No bots yet.",
    note: "The public page follows after the next daily collection and site publish.",
  },
  runs: {
    title: "Live runs",
    lead: (max: number) =>
      `The showcase bots that ran this config, each with every span it ran it; the ${max} that ran it most recently when there are more. Tick a bot to draw its runs with the backtest.`,
    clearAll: "Untick all",
    ongoing: "ongoing",
    caption: (c: { cap: string; start: string; end: string; days: number; ret: string }) =>
      `capital ${c.cap} · ${c.start} → ${c.end} · ${c.days} days · ${c.ret}`,
  },
};
