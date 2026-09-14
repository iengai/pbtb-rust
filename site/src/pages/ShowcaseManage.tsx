import { useState } from "react";
import { Link } from "react-router-dom";
import { api } from "../api/client";
import { useAction, useLoad } from "../api/hooks";
import { Crumbs, ErrorBanner, Loading, Pill } from "../components/ui";
import { useT } from "../i18n/locale";

// The operator's switchboard for the public showcase: every bot of their own
// with its Bybit link and whether the page shows it. The page itself is the
// collector's output, so a switch reaches it on the next daily run.
export function ShowcaseManage() {
  const t = useT();
  const me = useLoad(() => api.me(), "me");
  const operator = me.data?.role === "operator";
  const list = useLoad(
    () => (operator ? api.showcaseCandidates() : Promise.resolve(null)),
    operator ? "showcase:candidates" : "showcase:candidates:none",
  );
  const action = useAction();
  const [pending, setPending] = useState<string | null>(null);
  const bots = list.data?.bots ?? [];

  const crumbs = <Crumbs items={[{ to: "/p", label: t.showcase.title }, { label: t.showcase.manage.title }]} />;
  if (me.data && !operator) {
    return (
      <>
        {crumbs}
        <div className="msg">
          {t.showcase.manage.operatorOnly} <Link to="/p">{t.showcase.title}</Link>
        </div>
      </>
    );
  }

  return (
    <>
      {crumbs}
      <div className="page-head">
        <div>
          <h1>{t.showcase.manage.title}</h1>
          <div className="sub">{t.showcase.manage.lead}</div>
        </div>
      </div>
      <ErrorBanner error={me.error} onRetry={me.reload} />
      <ErrorBanner error={list.error} onRetry={list.reload} />
      <ErrorBanner error={action.error} onDismiss={action.clear} />
      {(me.loading || list.loading) && !list.data && <Loading what={t.showcase.manage.loading} />}
      {list.data && bots.length === 0 && <div className="msg">{t.showcase.manage.empty}</div>}
      {bots.length > 0 && (
        <div className="table showcase-manage">
          <div className="th">
            <div>{t.showcase.manage.col.bot}</div>
            <div>{t.showcase.manage.col.link}</div>
            <div>{t.showcase.manage.col.state}</div>
            <div />
          </div>
          {bots.map((b) => (
            <div key={b.bot_id} className="tr">
              <div>
                <div style={{ fontWeight: 600 }}>{b.name}</div>
                <div style={{ fontSize: 12.5, color: "var(--muted)" }}>{b.exchange.toUpperCase()}</div>
              </div>
              <div className="hide-sm ellipsis" style={{ fontSize: 13 }}>
                {b.public_url ? (
                  <a href={b.public_url} target="_blank" rel="noopener noreferrer">
                    {b.public_url} ↗
                  </a>
                ) : (
                  <span className="muted">{t.showcase.manage.noLink}</span>
                )}
              </div>
              <div>
                {b.public ? (
                  <Pill tone="ok">{t.showcase.manage.shown}</Pill>
                ) : (
                  <Pill>{t.showcase.manage.hidden}</Pill>
                )}
              </div>
              <div>
                <button
                  type="button"
                  className={`btn${b.public ? "" : " primary"}`}
                  disabled={action.busy}
                  onClick={() =>
                    void action.run(async () => {
                      setPending(b.bot_id);
                      try {
                        await api.setShowcase(b.bot_id, !b.public);
                        list.reload();
                      } finally {
                        setPending(null);
                      }
                    })
                  }
                >
                  {pending === b.bot_id ? "…" : b.public ? t.showcase.manage.hide : t.showcase.manage.show}
                </button>
              </div>
            </div>
          ))}
        </div>
      )}
      <div className="note">{t.showcase.manage.note}</div>
    </>
  );
}
