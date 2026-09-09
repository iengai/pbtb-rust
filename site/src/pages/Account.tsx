import { useState } from "react";
import { api } from "../api/client";
import { useAction, useLoad } from "../api/hooks";
import { useAuth } from "../auth/AuthProvider";
import { Google, Link } from "../components/icons";
import { ErrorBanner, Loading, Modal, Pill } from "../components/ui";
import { useT } from "../i18n/locale";

function maskId(id: string): string {
  if (id.length <= 5) return id;
  return `${id.slice(0, 3)}${"•".repeat(Math.max(3, id.length - 5))}${id.slice(-2)}`;
}

type Ticket = { token: string; url: string | null; expires_in: number };

export function Account() {
  const t = useT();
  const { session, signOut } = useAuth();
  const me = useLoad(() => api.me(), "me");
  const bots = useLoad(() => api.listBots(), "bots");
  const [unbinding, setUnbinding] = useState(false);
  const [ticket, setTicket] = useState<Ticket | null>(null);
  const action = useAction();

  const egress = import.meta.env.VITE_EGRESS_IP as string | undefined;
  const google = me.data?.identities.find((it) => it.provider === "workos");

  return (
    <>
      <h1 style={{ marginBottom: 18 }}>{t.account.title}</h1>
      <ErrorBanner error={me.error} onRetry={me.reload} />
      {me.loading && !me.data && <Loading what={t.account.loading} />}
      {me.data && (
        <div className="two-col">
          <div className="stack">
            <div className="card">
              <div className="card-title xs">{t.account.signIn.title}</div>
              <div className="sub" style={{ marginBottom: 8 }}>
                {t.account.signIn.lead}
              </div>
              <div className="list-row">
                <div style={{ display: "flex", alignItems: "center", gap: 12, minWidth: 0 }}>
                  <Google />
                  <div style={{ minWidth: 0 }}>
                    <div className="ellipsis" style={{ fontSize: 14, fontWeight: 550 }}>
                      {session?.claims.email ?? google?.subject ?? "—"}
                    </div>
                    <div className="hint">{t.account.signIn.via}</div>
                  </div>
                </div>
              </div>
            </div>
            <div className="card">
              <div className="card-title sm">{t.account.telegram.title}</div>
              <div className="sub" style={{ marginBottom: 10 }}>
                {t.account.telegram.lead}
              </div>
              {me.data.telegram ? (
                <div className="list-row" style={{ borderTop: "none", paddingTop: 0 }}>
                  <div style={{ display: "flex", alignItems: "center", gap: 12, minWidth: 0 }}>
                    <Link />
                    <div>
                      <div style={{ fontSize: 14, fontWeight: 550 }} className="tnum">
                        {maskId(me.data.telegram)}
                      </div>
                      <div className="hint">{t.account.telegram.bound}</div>
                    </div>
                  </div>
                  <button type="button" className="btn danger" onClick={() => setUnbinding(true)}>
                    {t.account.telegram.unbind}
                  </button>
                </div>
              ) : ticket ? (
                <div className="stack" style={{ gap: 10 }}>
                  <div style={{ fontSize: 14 }}>
                    {t.account.telegram.ticketLead(Math.round(ticket.expires_in / 60))}
                  </div>
                  {ticket.url ? (
                    <a className="btn" href={ticket.url} target="_blank" rel="noreferrer">
                      {t.account.telegram.open}
                    </a>
                  ) : (
                    <div className="mono" style={{ fontSize: 13, wordBreak: "break-all" }}>
                      /start {ticket.token}
                    </div>
                  )}
                  <div className="hint">
                    {ticket.url ? t.account.telegram.ifNotOpen : t.account.telegram.sendThis}
                  </div>
                  <div style={{ display: "flex", gap: 8 }}>
                    <button
                      type="button"
                      className="btn"
                      onClick={() => {
                        setTicket(null);
                        me.reload();
                      }}
                    >
                      {t.account.telegram.done}
                    </button>
                    <button type="button" className="btn ghost" onClick={() => setTicket(null)}>
                      {t.common.cancel}
                    </button>
                  </div>
                </div>
              ) : (
                <div className="stack" style={{ gap: 10 }}>
                  <div className="muted">{t.account.telegram.none}</div>
                  <ErrorBanner error={action.error} onDismiss={action.clear} />
                  <div>
                    <button
                      type="button"
                      className="btn"
                      disabled={action.busy}
                      onClick={() =>
                        void action.run(async () => {
                          setTicket(await api.bindTicket());
                        })
                      }
                    >
                      {t.account.telegram.bind}
                    </button>
                  </div>
                </div>
              )}
            </div>
          </div>
          <div className="stack">
            <div className="card">
              <div className="card-title sm">{t.account.summary.title}</div>
              <div className="kv wide">
                <div className="k">{t.account.summary.id}</div>
                <div className="tnum">{maskId(me.data.user_id)}</div>
                <div className="k">{t.account.summary.level}</div>
                <div style={{ display: "flex", gap: 8 }}>
                  <Pill tone="ok">{t.account.summary.vip(me.data.vip_level)}</Pill>
                </div>
                <div className="k">{t.account.summary.bots}</div>
                <div>{bots.data ? bots.data.bots.length : "…"}</div>
              </div>
            </div>
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
      {unbinding && (
        <Modal title={t.account.unbind.title} onClose={() => setUnbinding(false)}>
          <div style={{ fontSize: 14 }}>{t.account.unbind.body}</div>
          <ErrorBanner error={action.error} onDismiss={action.clear} />
          <div className="actions">
            <button type="button" className="btn ghost" onClick={() => setUnbinding(false)}>
              {t.common.cancel}
            </button>
            <button
              type="button"
              className="btn danger solid"
              disabled={action.busy}
              onClick={() =>
                void action.run(async () => {
                  await api.unbindTelegram();
                  setUnbinding(false);
                  me.reload();
                })
              }
            >
              {t.account.telegram.unbind}
            </button>
          </div>
        </Modal>
      )}
    </>
  );
}
