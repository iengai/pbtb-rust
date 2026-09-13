import { Navigate } from "react-router-dom";
import { useAuth } from "../auth/AuthProvider";
import { configured } from "../auth/oauth";
import { Google } from "../components/icons";
import { Notice } from "../components/ui";
import { useT } from "../i18n/locale";

export function Login() {
  const { session, reason, login } = useAuth();
  const t = useT();
  if (session) return <Navigate to="/bots" replace />;
  return (
    <div className="login-wrap framed">
      <div className="login">
        <div>
          <h1>{t.auth.signIn}</h1>
          <div className="lead">{t.auth.lead}</div>
        </div>
        {reason === "expired" && <Notice tone="info">{t.auth.expiredNotice}</Notice>}
        {reason === "scope" && <Notice tone="info">{t.auth.scopeNotice()}</Notice>}
        {!configured() && <Notice>{t.auth.notConfigured()}</Notice>}
        <button type="button" className="btn lg" onClick={() => void login()} disabled={!configured()}>
          <Google />
          {t.auth.continueWithGoogle}
        </button>
        <div className="fine">{t.auth.newHere}</div>
      </div>
    </div>
  );
}
