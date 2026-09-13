// The config-centric reading of a showcase bot's curve: one run per span the
// bot ran one config, re-based to its own start. Derived here from the public
// artifact (points, switches, resets) so the collector publishes one shape and
// the config page and the bot page read the same file.

import type { ShowcaseBot } from "../data/static";
import { type BotReturnSeries, type ViewPoint } from "./returnCurve";

const DAY = 86400;

// The config page shows the bots that ran a config most recently when many did.
export const MAX_BOTS_PER_TEMPLATE = 5;

// At or below this the account was wiped out; a run cannot start from it.
const DEAD_EPS = 1e-9;

export type Run = {
  bot: { id: string; name: string; public_url: string };
  template_name: string;
  /** The capital the bot ran this span at, rounded to a magnitude by the collector. */
  cap_usdt: number;
  start: number;
  /** null while the span is still running. */
  end: number | null;
  /** Calendar days from the span's start to its end (or the collection time). */
  days: number;
  return_pct: number;
  points: ViewPoint[];
};

type Span = { start: number; end: number | null; cap_usdt: number; fromReset: boolean };

// The public file read as the owner's series shape, so the same windowing
// draws both: only the resets differ, a bare timestamp on the private side.
export function asSeries(bot: ShowcaseBot): BotReturnSeries {
  return { ...bot, capital_resets: (bot.capital_resets ?? []).map((r) => r.ts) };
}

// Every run of every config on one bot, newest first. A span opens at a
// config switch and closes at the next; a capital reset inside it splits it,
// since the index restarts at 100 there and nothing may be re-based across.
// A run counts by the calendar, as the exchange counts: the bot may have
// been idle on some of the days.
export function deriveRuns(bot: ShowcaseBot, now: number = bot.generated_at): Run[] {
  const points = (bot.points ?? []).slice().sort((a, b) => a.ts - b.ts);
  const switches = (bot.config_switches ?? []).slice().sort((a, b) => a.ts - b.ts);
  const resets = (bot.capital_resets ?? []).slice().sort((a, b) => a.ts - b.ts);
  const runs: Run[] = [];

  switches.forEach((sw, i) => {
    const spanEnd = switches[i + 1]?.ts ?? null;
    const inner = resets.filter((r) => r.ts > sw.ts && (spanEnd == null || r.ts < spanEnd));
    let span: Span = { start: sw.ts, end: inner[0]?.ts ?? spanEnd, cap_usdt: sw.cap_usdt, fromReset: false };
    for (let k = 0; ; k++) {
      const run = runOf(bot, points, sw.template_name, span, now);
      if (run) runs.push(run);
      const reset = inner[k];
      if (!reset) break;
      span = { start: reset.ts, end: inner[k + 1]?.ts ?? spanEnd, cap_usdt: reset.cap_usdt, fromReset: true };
    }
  });

  return runs.sort((a, b) => b.start - a.start);
}

function runOf(bot: ShowcaseBot, points: ShowcaseBot["points"], template: string, span: Span, now: number): Run | null {
  const endTs = span.end ?? now;
  const days = Math.floor((endTs - span.start) / DAY);
  const inside = points.filter((p) => p.ts >= span.start && p.ts <= endTs);
  // A span with no daily close yet has nothing to draw.
  if (inside.length === 0) return null;
  // The base is the close the span started from: the last point before a
  // switch, the reset day's own point after a re-funding (the day before held
  // the dust the deposit replaced).
  const before = span.fromReset ? undefined : points.filter((p) => p.ts < span.start).pop();
  const base = (before ?? inside[0]!).index;
  if (!(base > DEAD_EPS)) return null;
  const view: ViewPoint[] = inside.map((p) => ({ ts: p.ts, return_pct: (p.index / base - 1) * 100 }));
  return {
    bot: { id: bot.id, name: bot.name, public_url: bot.public_url },
    template_name: template,
    cap_usdt: span.cap_usdt,
    start: span.start,
    end: span.end,
    days,
    return_pct: view[view.length - 1]!.return_pct,
    points: view,
  };
}

/** Every run of one config on one bot, newest first. */
export type BotRuns = { bot: Run["bot"]; runs: Run[] };

// The showcase bots that ran one config, each with all its runs of it: a bot
// that left the config and came back is one entry, not several. Ordered by
// each bot's newest run, capped by bot.
export function botsForTemplate(bots: ShowcaseBot[], template: string, limit = MAX_BOTS_PER_TEMPLATE): BotRuns[] {
  return bots
    .map((b) => ({ runs: deriveRuns(b).filter((r) => r.template_name === template) }))
    .filter((g) => g.runs.length > 0)
    .map(({ runs }) => ({ bot: runs[0]!.bot, runs }))
    .sort((a, b) => b.runs[0]!.start - a.runs[0]!.start)
    .slice(0, limit);
}

// "$500", "$1k", "$1.5k", "$20k": the collector's rounded capital, in the
// magnitude it rounds to.
export function fmtCap(v: number): string {
  if (!(v > 0)) return "—";
  if (v >= 1_000_000) return `$${(v / 1_000_000).toFixed(v % 1_000_000 ? 1 : 0)}M`;
  if (v >= 1000) return `$${(v / 1000).toFixed(v % 1000 ? 1 : 0)}k`;
  return `$${v.toFixed(0)}`;
}
