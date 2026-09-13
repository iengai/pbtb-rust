import { Link } from "react-router-dom";
import { useLoad } from "../api/hooks";
import { useRangeLabel } from "../chart/ReturnChart";
import { DEFAULT_RANGE, RANGES, fmtDate, fmtSignedPct } from "../chart/returnCurve";
import { headline } from "../chart/showcase";
import { Chevron } from "../components/icons";
import { ErrorBanner, Loading, Sparkline } from "../components/ui";
import { staticData } from "../data/static";
import { useT } from "../i18n/locale";

// The public listing: the operator's showcase bots, from the static index
// the collector publishes. No token, no money — percentages and a link to
// each bot's copy-trading page on the exchange. Each row reads the bot's full
// series so its figure and trend are the default window's, as on the bot page.
export function Showcase() {
  const t = useT();
  const rangeLabel = useRangeLabel();
  const range = RANGES[DEFAULT_RANGE]!;
  const { data, error, loading, reload } = useLoad(async () => {
    const index = await staticData.showcase();
    if (!index) return null;
    // A bot file that fails to load costs its own row's figure, not the listing.
    const series = await Promise.all(index.bots.map((b) => staticData.showcaseBot(b.id).catch(() => null)));
    return { index, heads: series.map((s) => (s ? headline(s) : null)) };
  }, "showcase");
  const bots = data?.index.bots ?? [];
  const heads = data?.heads ?? [];

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
            <div>{t.showcase.col.trend(range.days ?? 0)}</div>
            <div>{t.returns.tile.return(range.k)}</div>
            <div />
            <div />
          </div>
          {bots.map((b, i) => {
            const head = heads[i] ?? null;
            const up = (head?.ret ?? 0) >= 0;
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
                  <Sparkline pts={(head?.view ?? []).map((p) => ({ ts: p.ts, v: p.return_pct }))} up={up} />
                </div>
                <div
                  className="tnum"
                  style={{ fontWeight: 600, color: head ? (up ? "var(--pnl)" : "var(--pnl-neg)") : "var(--muted)" }}
                >
                  {head ? fmtSignedPct(head.ret) : "—"}
                  {head?.label.kind === "sinceRefunding" && (
                    <div style={{ fontSize: 12, fontWeight: 400, color: "var(--muted)" }}>{rangeLabel(head.label)}</div>
                  )}
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
          {t.showcase.disclaimer} · {t.showcase.updated(fmtDate(data.index.generated_at))}
        </div>
      )}
    </>
  );
}
