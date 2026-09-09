import { Navigate } from "react-router-dom";
import { useAuth } from "../auth/AuthProvider";
import { configured } from "../auth/oauth";
import { Google, Link } from "../components/icons";
import { Notice } from "../components/ui";

export function Login() {
  const { session, reason, login } = useAuth();
  if (session) return <Navigate to="/bots" replace />;
  return (
    <div className="login-wrap">
      <div className="login">
        <div className="brand">
          <div className="logo" />
          <div className="name">PBTB Console</div>
        </div>
        <div>
          <h1>Sign in</h1>
          <div className="lead">Manage your Passivbot bots from the browser.</div>
        </div>
        {reason === "unlinked" && (
          <Notice icon={<Link />}>
            <div style={{ fontWeight: 600 }}>This Google account is not linked to a bot account yet.</div>
            <div style={{ marginTop: 4 }}>
              Open the Telegram bot, press <b>Link account</b>, and sign in with the same Google account.
              Then come back and sign in again.
            </div>
          </Notice>
        )}
        {reason === "expired" && <Notice tone="info">Your session has expired. Sign in again to continue.</Notice>}
        {reason === "scope" && (
          <Notice tone="info">
            The last sign-in did not grant both <span className="mono">bots:read</span> and{" "}
            <span className="mono">bots:write</span>. Sign in again and accept both.
          </Notice>
        )}
        {!configured() && (
          <Notice>
            The console is not configured: set <span className="mono">VITE_API_URL</span>,{" "}
            <span className="mono">VITE_OAUTH_ISSUER</span> and <span className="mono">VITE_OAUTH_CLIENT_ID</span>{" "}
            (see <span className="mono">.env.example</span>).
          </Notice>
        )}
        <button type="button" className="btn lg" onClick={() => void login()} disabled={!configured()}>
          <Google />
          Continue with Google
        </button>
        <div className="fine">
          Access is by invitation: your Google account must first be linked from the Telegram bot. There is
          no sign-up here.
        </div>
      </div>
    </div>
  );
}
