// The account page: linked sign-ins, the Telegram account behind them, the
// egress address to whitelist, and the current session.
export const account = {
  title: "Account",
  loading: "account",
  identities: {
    title: "Linked identities",
    lead: () => (
      <>
        Sign-ins that map to this Telegram account. Unlinking one signs it out everywhere. To link
        another sign-in, press <b>Link account</b> in the Telegram bot; there is no link flow here.
      </>
    ),
    none: "No linked identities.",
    via: (provider: string, current: boolean) =>
      `Google via ${provider}${current ? " · this session" : ""}`,
    unlink: "Unlink",
  },
  telegram: {
    title: "Telegram account",
    userId: "User id",
    allowlist: "Allowlist",
    allowed: "Allowed",
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
  unlinkAll: {
    title: "Unlink all identities?",
    body: "Every sign-in linked to this Telegram account is released, including the one you are using now. You are signed out here, and can link again from the Telegram bot.",
  },
};
