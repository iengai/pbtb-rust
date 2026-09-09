import { Navigate, useNavigate } from "react-router-dom";
import { api } from "../api/client";
import { useAction } from "../api/hooks";
import { useAuth } from "../auth/AuthProvider";
import { Google } from "../components/icons";
import { ErrorBanner, Notice } from "../components/ui";
import { LangSwitch, useT } from "../i18n/locale";

// A signed-in Google account that has no account here yet. Creating one is an
// explicit click, never a side effect of signing in: authenticating with
// Google is not, by itself, an account.
export function Signup() {
  const { session, reason, clearReason, signOut } = useAuth();
  const navigate = useNavigate();
  const action = useAction();
  const t = useT();
  if (!session) return <Navigate to="/" replace />;
  if (reason !== "unlinked") return <Navigate to="/bots" replace />;

  const who = session.claims.email ?? session.claims.sub ?? t.auth.signup.thisAccount;
  return (
    <div className="login-wrap">
      <div className="login">
        <div className="brand">
          <div className="logo" />
          <div className="name">PBTB Console</div>
          <LangSwitch />
        </div>
        <div>
          <h1>{t.auth.signup.title}</h1>
          <div className="lead">{t.auth.signup.lead(who)}</div>
        </div>
        <Notice icon={<Google />}>
          <div style={{ fontWeight: 600 }}>{t.auth.signup.whatTitle}</div>
          <div style={{ marginTop: 4 }}>{t.auth.signup.whatBody}</div>
        </Notice>
        <ErrorBanner error={action.error} onDismiss={action.clear} />
        <button
          type="button"
          className="btn lg"
          disabled={action.busy}
          onClick={() =>
            void action.run(async () => {
              await api.signup();
              clearReason();
              navigate("/bots", { replace: true });
            })
          }
        >
          {t.auth.signup.create}
        </button>
        <div className="fine">
          {t.auth.signup.notYou}{" "}
          <button type="button" className="btn link" onClick={signOut}>
            {t.auth.signup.signOut}
          </button>{" "}
          {t.auth.signup.andSignIn}
        </div>
      </div>
    </div>
  );
}
