import { useState } from "react";
import { api } from "../api/client";
import { useAction, useLoad } from "../api/hooks";
import { useAuth } from "../auth/AuthProvider";
import { Google } from "../components/icons";
import { ErrorBanner, Loading, Modal, Pill } from "../components/ui";

function maskUserId(id: string): string {
  if (id.length <= 5) return id;
  return `${id.slice(0, 3)}${"•".repeat(Math.max(3, id.length - 5))}${id.slice(-2)}`;
}

export function Account() {
  const { session, signOut } = useAuth();
  const me = useLoad(() => api.me(), "me");
  const bots = useLoad(() => api.listBots(), "bots");
  const [unlinking, setUnlinking] = useState(false);
  const action = useAction();

  const egress = import.meta.env.VITE_EGRESS_IP as string | undefined;
  const mySub = session?.claims.sub;

  return (
    <>
      <h1 style={{ marginBottom: 18 }}>Account</h1>
      <ErrorBanner error={me.error} onRetry={me.reload} />
      {me.loading && !me.data && <Loading what="account" />}
      {me.data && (
        <div className="two-col">
          <div className="stack">
            <div className="card">
              <div className="card-title xs">Linked identities</div>
              <div className="sub" style={{ marginBottom: 8 }}>
                Sign-ins that map to this Telegram account. Unlinking one signs it out everywhere. To link
                another sign-in, press <b>Link account</b> in the Telegram bot; there is no link flow here.
              </div>
              {me.data.identities.length === 0 && <div className="muted">No linked identities.</div>}
              {me.data.identities.map((it) => {
                const mine = it.subject === mySub;
                return (
                  <div key={`${it.provider}:${it.subject}`} className="list-row">
                    <div style={{ display: "flex", alignItems: "center", gap: 12, minWidth: 0 }}>
                      <Google />
                      <div style={{ minWidth: 0 }}>
                        <div className="ellipsis" style={{ fontSize: 14, fontWeight: 550 }}>
                          {mine && session?.claims.email ? session.claims.email : it.subject}
                        </div>
                        <div className="hint">
                          Google via {it.provider}
                          {mine ? " · this session" : ""}
                        </div>
                      </div>
                    </div>
                    <button type="button" className="btn danger" onClick={() => setUnlinking(true)}>
                      Unlink
                    </button>
                  </div>
                );
              })}
            </div>
            <div className="card">
              <div className="card-title sm">Telegram account</div>
              <div className="kv wide">
                <div className="k">User id</div>
                <div className="tnum">{maskUserId(me.data.user_id)}</div>
                <div className="k">Allowlist</div>
                <div style={{ display: "flex", gap: 8 }}>
                  <Pill tone="ok">Allowed</Pill>
                </div>
                <div className="k">Bots</div>
                <div>{bots.data ? bots.data.bots.length : "…"}</div>
              </div>
            </div>
          </div>
          <div className="stack">
            <div className="card">
              <div className="card-title sm">Exchange egress address</div>
              <div className="tnum mono" style={{ fontSize: 14 }}>
                {egress || "Not published"}
              </div>
              <div className="hint" style={{ marginTop: 6 }}>
                {egress
                  ? "Whitelist this IP on every Bybit API key you add."
                  : "Ask the operator for the NAT egress IP and whitelist it on every Bybit API key you add."}
              </div>
            </div>
            <div className="card">
              <div className="card-title sm">Session</div>
              <div className="sub" style={{ marginBottom: 10 }}>
                Scopes: {me.data.scopes.join(", ") || "none"} · renewed in the background until you sign
                out
              </div>
              <button type="button" className="btn" onClick={signOut}>
                Sign out
              </button>
            </div>
          </div>
        </div>
      )}
      {unlinking && (
        <Modal title="Unlink all identities?" onClose={() => setUnlinking(false)}>
          <div style={{ fontSize: 14 }}>
            Every sign-in linked to this Telegram account is released, including the one you are using now.
            You are signed out here, and can link again from the Telegram bot.
          </div>
          <ErrorBanner error={action.error} onDismiss={action.clear} />
          <div className="actions">
            <button type="button" className="btn ghost" onClick={() => setUnlinking(false)}>
              Cancel
            </button>
            <button
              type="button"
              className="btn danger solid"
              disabled={action.busy}
              onClick={() =>
                void action.run(async () => {
                  await api.unlinkIdentities();
                  signOut();
                })
              }
            >
              Unlink
            </button>
          </div>
        </Modal>
      )}
    </>
  );
}
