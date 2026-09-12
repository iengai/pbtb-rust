import { Link } from "react-router-dom";
import { useLoad } from "../api/hooks";
import { fmtDate, fmtSignedPct } from "../chart/returnCurve";
import { Chevron } from "../components/icons";
import { ErrorBanner, Loading, Sparkline } from "../components/ui";
import { staticData } from "../data/static";
import { useT } from "../i18n/locale";

// The public listing: the operator's showcase bots, from the static index
// the collector publishes. No token, no money — percentages and a link to
// each bot's copy-trading page on the exchange.
export function Showcase() {
  const t = useT();
  const { data, error, loading, reload } = useLoad(() => staticData.showcase(), "showcase");
  const bots = data?.bots ?? [];

  return (
    <>
      <div className="page-head">
        <div>
          <h1>{t.showcase.title}</h1>
          <div className="sub">{t.showcase.lead}</div>
        </div>
      </div>
      <ErrorBanner error={error} onRetry={reload} />
      {loading && !data && <Loading what={t.showcase.loading} />}
      {!loading && !error && bots.length === 0 && <div className="msg">{t.showcase.nothing}</div>}
      {bots.length > 0 && (
        <div className="table showcase">
          <div className="th">
            <div>{t.showcase.col.bot}</div>
            <div>{t.showcase.col.trend}</div>
            <div>{t.showcase.col.current}</div>
            <div />
            <div />
          </div>
          {bots.map((b) => {
            const up = b.current_return_pct >= 0;
            const to = `/p/bots/${encodeURIComponent(b.id)}`;
            return (
              <div key={b.id} className="tr">
                <div>
                  <Link to={to} style={{ fontWeight: 600, color: "var(--text)" }}>
                    {b.name}
                  </Link>
                  <div style={{ fontSize: 12.5, color: "var(--muted)" }}>{b.exchange.toUpperCase()}</div>
                </div>
                <div className="hide-sm">
                  <Sparkline pts={b.spark.map((v, i) => ({ ts: i, v }))} up={up} />
                </div>
                <div className="tnum" style={{ fontWeight: 600, color: up ? "var(--pnl)" : "var(--pnl-neg)" }}>
                  {fmtSignedPct(b.current_return_pct)}
                </div>
                <div style={{ fontSize: 13 }}>
                  <a href={b.public_url} target="_blank" rel="noopener noreferrer">
                    {t.showcase.onBybit} ↗
                  </a>
                </div>
                <Link to={to} style={{ color: "var(--muted)" }} aria-label={b.name}>
                  <Chevron />
                </Link>
              </div>
            );
          })}
        </div>
      )}
      {data && (
        <div className="note">
          {t.showcase.disclaimer} · {t.showcase.updated(fmtDate(data.generated_at))}
        </div>
      )}
    </>
  );
}
