// The account page: the fixed Google sign-in, the Telegram account bound to
// it, the level, the egress address to whitelist, and the current session.
export const account = {
  title: "Account",
  loading: "account",
  signIn: {
    title: "Sign-in",
    lead: "The Google account this account was created with. It is the account's identity and cannot be changed or unlinked.",
    via: "Google via WorkOS",
  },
  telegram: {
    title: "Telegram",
    lead: "The Telegram account that may drive your bots from the bot chat. One per account; unbind to bind another.",
    bound: "Telegram user id · bound",
    unbind: "Unbind",
    none: "No Telegram account is bound.",
    bind: "Bind a Telegram account",
    ticketLead: (minutes: number) =>
      `Open the link below from the Telegram account you want to bind. It works once and expires in ${minutes} minutes.`,
    open: "Open in Telegram",
    ifNotOpen: "If the link does not open, send the bot the /start command it carries.",
    sendThis: "Send this to the bot in a private chat.",
    done: "Done — check binding",
  },
  summary: {
    title: "Account",
    id: "Account id",
    level: "Level",
    vip: (level: number) => `VIP ${level}`,
    bots: "Bots",
  },
  egress: {
    title: "Exchange egress address",
    unpublished: "Not published",
    hint: "Whitelist this IP on every Bybit API key you add.",
    askOperator:
      "Ask the operator for the NAT egress IP and whitelist it on every Bybit API key you add.",
  },
  session: {
    title: "Session",
    scopes: (scopes: string) => `Scopes: ${scopes} · renewed in the background until you sign out`,
    noScopes: "none",
    signOut: "Sign out",
  },
  unbind: {
    title: "Unbind Telegram?",
    body: "The bound Telegram account stops being able to drive your bots. Your account, bots and this sign-in are untouched; you can bind another Telegram account afterwards.",
  },
};
