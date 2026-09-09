import { useEffect, useRef, useState } from "react";
import { Link, useLocation, useNavigate } from "react-router-dom";
import { api, AuthRefused } from "../api/client";
import { useAuth } from "../auth/AuthProvider";
import { completeLogin } from "../auth/oauth";
import { Notice } from "../components/ui";

// The OAuth redirect target: trades the code for a token, then asks the API
// who the token is before showing anything — a verified token nobody has
// linked belongs on the login page, not on an empty bot list.
export function Callback() {
  const { search } = useLocation();
  const navigate = useNavigate();
  const { adopt, refuse } = useAuth();
  const [error, setError] = useState<string | null>(null);
  const ran = useRef(false);

  useEffect(() => {
    if (ran.current) return; // StrictMode mounts twice; the code is single-use
    ran.current = true;
    (async () => {
      try {
        const session = await completeLogin(search);
        adopt(session);
        await api.me();
        navigate("/bots", { replace: true });
      } catch (e) {
        if (e instanceof AuthRefused) {
          refuse(e.kind);
          navigate("/", { replace: true });
          return;
        }
        setError(e instanceof Error ? e.message : String(e));
      }
    })();
  }, [search, adopt, refuse, navigate]);

  return (
    <div className="login-wrap">
      <div className="login">
        <div className="brand">
          <div className="logo" />
          <div className="name">PBTB Console</div>
        </div>
        {error ? (
          <>
            <Notice>
              <div style={{ fontWeight: 600 }}>Sign-in did not complete.</div>
              <div style={{ marginTop: 4 }}>{error}</div>
            </Notice>
            <Link to="/" className="btn">
              Back to sign in
            </Link>
          </>
        ) : (
          <div className="muted">Completing sign-in…</div>
        )}
      </div>
    </div>
  );
}
