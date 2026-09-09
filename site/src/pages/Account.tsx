import { useState } from "react";
import { api } from "../api/client";
import { useAction, useLoad } from "../api/hooks";
import { useAuth } from "../auth/AuthProvider";
import { Google } from "../components/icons";
import { ErrorBanner, Loading, Modal, Pill } from "../components/ui";
import { useT } from "../i18n/locale";

function maskUserId(id: string): string {
  if (id.length <= 5) return id;
  return `${id.slice(0, 3)}${"•".repeat(Math.max(3, id.length - 5))}${id.slice(-2)}`;
}

export function Account() {
  const t = useT();
  const { session, signOut } = useAuth();
  const me = useLoad(() => api.me(), "me");
  const bots = useLoad(() => api.listBots(), "bots");
  const [unlinking, setUnlinking] = useState(false);
  const action = useAction();

  const egress = import.meta.env.VITE_EGRESS_IP as string | undefined;
  const mySub = session?.claims.sub;

  return (
    <>
      <h1 style={{ marginBottom: 18 }}>{t.account.title}</h1>
      <ErrorBanner error={me.error} onRetry={me.reload} />
      {me.loading && !me.data && <Loading what={t.account.loading} />}
      {me.data && (
        <div className="two-col">
          <div className="stack">
            <div className="card">
              <div className="card-title xs">{t.account.identities.title}</div>
              <div className="sub" style={{ marginBottom: 8 }}>
                {t.account.identities.lead()}
              </div>
              {me.data.identities.length === 0 && <div className="muted">{t.account.identities.none}</div>}
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
                        <div className="hint">{t.account.identities.via(it.provider, mine)}</div>
                      </div>
                    </div>
                    <button type="button" className="btn danger" onClick={() => setUnlinking(true)}>
                      {t.account.identities.unlink}
                    </button>
                  </div>
                );
              })}
            </div>
            <div className="card">
              <div className="card-title sm">{t.account.telegram.title}</div>
              <div className="kv wide">
                <div className="k">{t.account.telegram.userId}</div>
                <div className="tnum">{maskUserId(me.data.user_id)}</div>
                <div className="k">{t.account.telegram.allowlist}</div>
                <div style={{ display: "flex", gap: 8 }}>
                  <Pill tone="ok">{t.account.telegram.allowed}</Pill>
                </div>
                <div className="k">{t.account.telegram.bots}</div>
                <div>{bots.data ? bots.data.bots.length : "…"}</div>
              </div>
            </div>
          </div>
          <div className="stack">
            <div className="card">
              <div className="card-title sm">{t.account.egress.title}</div>
              <div className="tnum mono" style={{ fontSize: 14 }}>
                {egress || t.account.egress.unpublished}
              </div>
              <div className="hint" style={{ marginTop: 6 }}>
                {egress ? t.account.egress.hint : t.account.egress.askOperator}
              </div>
            </div>
            <div className="card">
              <div className="card-title sm">{t.account.session.title}</div>
              <div className="sub" style={{ marginBottom: 10 }}>
                {t.account.session.scopes(me.data.scopes.join(", ") || t.account.session.noScopes)}
              </div>
              <button type="button" className="btn" onClick={signOut}>
                {t.account.session.signOut}
              </button>
            </div>
          </div>
        </div>
      )}
      {unlinking && (
        <Modal title={t.account.unlinkAll.title} onClose={() => setUnlinking(false)}>
          <div style={{ fontSize: 14 }}>{t.account.unlinkAll.body}</div>
          <ErrorBanner error={action.error} onDismiss={action.clear} />
          <div className="actions">
            <button type="button" className="btn ghost" onClick={() => setUnlinking(false)}>
              {t.common.cancel}
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
              {t.account.identities.unlink}
            </button>
          </div>
        </Modal>
      )}
    </>
  );
}
