// The screens that render outside the layout: the login page, the signup
// page and the OAuth redirect target.
export const auth = {
  signIn: "Sign in",
  lead: "Manage your Passivbot bots from the browser.",
  expiredNotice: "Your session has expired. Sign in again to continue.",
  scopeNotice: () => (
    <>
      The last sign-in did not grant both <span className="mono">bots:read</span> and{" "}
      <span className="mono">bots:write</span>. Sign in again and accept both.
    </>
  ),
  notConfigured: () => (
    <>
      The console is not configured: set <span className="mono">VITE_API_URL</span>,{" "}
      <span className="mono">VITE_OAUTH_ISSUER</span> and <span className="mono">VITE_OAUTH_CLIENT_ID</span>{" "}
      (see <span className="mono">.env.example</span>).
    </>
  ),
  continueWithGoogle: "Continue with Google",
  newHere:
    "New here? Continue with Google and create your account on the next page. Telegram is bound from the account page afterwards.",
  callbackFailed: "Sign-in did not complete.",
  backToSignIn: "Back to sign in",
  completing: "Completing sign-in…",
  signup: {
    title: "Create your account",
    lead: (who: string) => (
      <>
        You are signed in as <b>{who}</b>, which has no account here yet.
      </>
    ),
    whatTitle: "What you get",
    whatBody:
      "A tenant of your own, at level 0: add a bot with your own exchange keys, pick a config, and run one bot at a time. Binding a Telegram account is optional and done from the account page afterwards.",
    create: "Create account",
    notYou: "Not you?",
    signOut: "Sign out",
    andSignIn: "and sign in with another Google account.",
    thisAccount: "this Google account",
  },
};
