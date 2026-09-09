// The two screens that render outside the layout: the login page and the OAuth
// redirect target.
export const auth = {
  signIn: "Sign in",
  lead: "Manage your Passivbot bots from the browser.",
  unlinkedTitle: "This Google account is not linked to a bot account yet.",
  unlinkedBody: () => (
    <>
      Open the Telegram bot, press <b>Link account</b>, and sign in with the same Google account. Then come
      back and sign in again.
    </>
  ),
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
  invitationOnly:
    "Access is by invitation: your Google account must first be linked from the Telegram bot. There is no sign-up here.",
  callbackFailed: "Sign-in did not complete.",
  backToSignIn: "Back to sign in",
  completing: "Completing sign-in…",
};
