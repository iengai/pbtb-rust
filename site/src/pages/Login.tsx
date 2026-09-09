import { Navigate } from "react-router-dom";
import { useAuth } from "../auth/AuthProvider";
import { configured } from "../auth/oauth";
import { Google, Link } from "../components/icons";
import { Notice } from "../components/ui";
import { LangSwitch, useT } from "../i18n/locale";

export function Login() {
  const { session, reason, login } = useAuth();
  const t = useT();
  if (session) return <Navigate to="/bots" replace />;
  return (
    <div className="login-wrap">
      <div className="login">
        <div className="brand">
          <div className="logo" />
          <div className="name">PBTB Console</div>
          <LangSwitch />
        </div>
        <div>
          <h1>{t.auth.signIn}</h1>
          <div className="lead">{t.auth.lead}</div>
        </div>
        {reason === "unlinked" && (
          <Notice icon={<Link />}>
            <div style={{ fontWeight: 600 }}>{t.auth.unlinkedTitle}</div>
            <div style={{ marginTop: 4 }}>{t.auth.unlinkedBody()}</div>
          </Notice>
        )}
        {reason === "expired" && <Notice tone="info">{t.auth.expiredNotice}</Notice>}
        {reason === "scope" && <Notice tone="info">{t.auth.scopeNotice()}</Notice>}
        {!configured() && <Notice>{t.auth.notConfigured()}</Notice>}
        <button type="button" className="btn lg" onClick={() => void login()} disabled={!configured()}>
          <Google />
          {t.auth.continueWithGoogle}
        </button>
        <div className="fine">{t.auth.invitationOnly}</div>
      </div>
    </div>
  );
}
