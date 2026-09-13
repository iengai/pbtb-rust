// Strings shared by more than one area: navigation, phase labels, the generic
// buttons and the two states every page can be in.
export const common = {
  nav: {
    bots: "Bots",
    configs: "Configs",
    returns: "Returns",
    showcase: "Showcase",
    account: "Account",
    signIn: "Sign in",
  },
  phase: {
    running: "Running",
    starting: "Starting",
    stopping: "Stopping",
    stopped: "Stopped",
    unknown: "Unknown",
  },
  retry: "Retry",
  dismiss: "Dismiss",
  cancel: "Cancel",
  close: "Close",
  back: "Back",
  save: "Save",
  confirm: "Confirm",
  delete: "Delete",
  loading: "Loading…",
  loadingWhat: (what: string) => `Loading ${what}…`,
  networkError: "Network error — the console could not reach the API.",
  notFound: "Nothing here.",
  backToBots: "Back to bots",
  backToShowcase: "Back to the showcase",
  busyConflict: (phase: string) => `The bot is ${phase} right now — try again in a moment.`,
  // Refusals about the account's level (403 with a code), never about the session.
  insufficientLevel: (required: number, current: number) =>
    `This config needs VIP ${required}; your account is VIP ${current}.`,
  quotaExceeded: (limit: number) =>
    `Your level allows ${limit} running bot${limit === 1 ? "" : "s"} at a time — stop one first.`,
};
