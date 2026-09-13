import { useMemo, useState } from "react";
import { Link } from "react-router-dom";
import { useLoad } from "../api/hooks";
import { Brush, ComboChart, PresetBar } from "../chart/ComboChart";
import {
  type ComboSeries,
  RUN_COLORS,
  type Scale,
  type Span,
  domainOf,
  initialSelection,
  presetSpan,
} from "../chart/combo";
import { fmtDate, fmtSignedPct } from "../chart/returnCurve";
import { type BotRuns, MAX_BOTS_PER_TEMPLATE, botsForTemplate, fmtCap } from "../chart/showcase";
import { type TemplateBacktest, staticData } from "../data/static";
import { useT } from "../i18n/locale";

type ListedBot = BotRuns & { key: string; color: string };

// A config's chart card: the backtest and the live runs of the config on the
// showcase bots on one chart, a period to choose, and the bot list with a
// checkbox per bot that draws all of that bot's runs. The bot with the newest
// run is drawn first; a config no public bot has run shows the backtest alone
// and no list.
export function ConfigChart({ template }: { template: TemplateBacktest }) {
  const t = useT();
  const bots = useLoad(() => staticData.showcaseBots(), "showcase:bots");
  const listed = useMemo<ListedBot[]>(
    () =>
      (bots.data ? botsForTemplate(bots.data, template.name) : []).map((g, i) => ({
        ...g,
        key: g.bot.id,
        color: RUN_COLORS[i % RUN_COLORS.length]!,
      })),
    [bots.data, template.name],
  );
  // Untouched, the selection is the bot with the newest run; a click takes over from there.
  const [picked, setPicked] = useState<Set<string> | null>(null);
  const selected = picked ?? initialSelection(listed);

  const backtest = useMemo<ComboSeries[]>(
    () => [
      {
        id: "equity",
        label: t.configs.chart.backtest,
        color: "var(--accent)",
        points: template.points.map((p) => ({ ts: p.ts, v: p.equity })),
      },
      {
        id: "balance",
        label: t.configs.chart.balance,
        color: "var(--balance)",
        dashed: true,
        points: template.points.map((p) => ({ ts: p.ts, v: p.balance })),
      },
    ],
    [template.points, t],
  );
  // One series per run, in its bot's colour: each run is indexed from the
  // close before its own switch, so a bot's runs cannot be joined into one
  // line. That base is the series' first point, so a period opening before
  // the run re-bases it where the caption does.
  const live = useMemo<{ key: string; series: ComboSeries }[]>(
    () =>
      listed.flatMap((g) =>
        g.runs.map((r) => ({
          key: g.key,
          series: {
            id: `${g.key}:${r.start}`,
            label: g.bot.name,
            color: g.color,
            points: [{ ts: r.start, v: 1 }, ...r.points.map((p) => ({ ts: p.ts, v: 1 + p.return_pct / 100 }))],
          },
        })),
      ),
    [listed],
  );
  // The domain takes every run, ticked or not, so toggling one never moves
  // the axis under the reader.
  const domain = useMemo(() => domainOf([...backtest, ...live.map((l) => l.series)]), [backtest, live]);
  const [chosen, setChosen] = useState<Span | null>(null);
  // Log by default: a backtest that multiplies its capital is read as
  // multiples, and over a short period the two scales draw the same line.
  const [scale, setScale] = useState<Scale>("log");
  const span = chosen && domain ? chosen : domain ? presetSpan(domain, null) : null;
  const shown = useMemo(
    () => [...backtest, ...live.filter((l) => selected.has(l.key)).map((l) => l.series)],
    [backtest, live, selected],
  );

  const toggle = (key: string) =>
    setPicked((prev) => {
      const next = new Set(prev ?? selected);
      if (next.has(key)) next.delete(key);
      else next.add(key);
      return next;
    });

  return (
    <div className="card tight" style={{ marginBottom: 18 }}>
      <div className="card-title" style={{ marginBottom: 4, padding: "0 4px" }}>
        {t.configs.chart.title}
      </div>
      <div className="hint" style={{ marginBottom: 8, padding: "0 4px" }}>
        {t.configs.chart.hint}
      </div>
      {domain && span ? (
        <>
          <PresetBar domain={domain} span={span} onSpan={setChosen} scale={scale} onScale={setScale} />
          <ComboChart series={shown} span={span} scale={scale} />
          <Brush overview={backtest[0]!} domain={domain} span={span} onSpan={setChosen} />
        </>
      ) : (
        <div className="msg">{t.configs.chart.notEnough}</div>
      )}
      <div className="legend">
        <span>
          <i className="swatch" style={{ background: "var(--accent)" }} /> {t.configs.chart.backtest}
        </span>
        <span>
          <i className="swatch dash" /> {t.configs.chart.balance}
        </span>
      </div>
      {listed.length > 0 && (
        <div style={{ marginTop: 14, padding: "0 4px" }}>
          <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: 8 }}>
            <div className="card-title sm">{t.showcase.runs.title}</div>
            <button
              type="button"
              className="btn ghost act"
              style={{ height: 30 }}
              disabled={selected.size === 0}
              onClick={() => setPicked(new Set())}
            >
              {t.showcase.runs.clearAll}
            </button>
          </div>
          <div className="hint" style={{ marginBottom: 6 }}>
            {t.showcase.runs.lead(MAX_BOTS_PER_TEMPLATE)}
          </div>
          {listed.map(({ bot, runs, key, color }) => (
            <div key={key} className="run-row">
              <label>
                <input type="checkbox" checked={selected.has(key)} onChange={() => toggle(key)} />
                <i className="swatch" style={{ background: color }} />
              </label>
              <Link to={`/p/bots/${encodeURIComponent(bot.id)}`} style={{ fontWeight: 600, color: "var(--text)" }}>
                {bot.name}
              </Link>
              <a href={bot.public_url} target="_blank" rel="noopener noreferrer" style={{ fontSize: 13 }}>
                {t.showcase.onBybit} ↗
              </a>
              <div className="run-segs">
                {runs.map((r) => (
                  <span
                    key={r.start}
                    className="hint"
                    style={{ color: r.return_pct >= 0 ? "var(--pnl)" : "var(--pnl-neg)" }}
                  >
                    {t.showcase.runs.caption({
                      cap: fmtCap(r.cap_usdt),
                      start: fmtDate(r.start),
                      end: r.end != null ? fmtDate(r.end) : t.showcase.runs.ongoing,
                      days: r.days,
                      ret: fmtSignedPct(r.return_pct),
                    })}
                  </span>
                ))}
              </div>
            </div>
          ))}
          <div className="note" style={{ fontSize: 12, marginTop: 8 }}>
            {t.showcase.disclaimer}
          </div>
        </div>
      )}
    </div>
  );
}
