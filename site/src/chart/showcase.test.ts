import { describe, expect, it } from "vitest";
import type { ShowcaseBot } from "../data/static";
import { selectWindow } from "./returnCurve";
import { MAX_RUNS_PER_TEMPLATE, asSeries, deriveRuns, fmtCap, runsForTemplate } from "./showcase";

const DAY = 86400;

// A bot with one point per day at midnight, its index climbing 1% a day from
// day 0; the collection time is the last point.
function bot(days: number, overrides: Partial<ShowcaseBot> = {}): ShowcaseBot {
  const points = Array.from({ length: days }, (_, d) => ({
    ts: d * DAY,
    index: Number((100 * 1.01 ** d).toFixed(4)),
    return_pct: 0,
  }));
  return {
    id: "abc123def456",
    name: "Shown",
    exchange: "bybit",
    public_url: "https://www.bybit.com/copyTrade/x",
    generated_at: (days - 1) * DAY,
    current_return_pct: 0,
    points,
    config_switches: [],
    capital_resets: [],
    ...overrides,
  };
}

// A switch an hour into the day, as a real one lands between two closes.
const sw = (day: number, template: string, cap = 1000) => ({ ts: day * DAY + 3600, template_name: template, cap_usdt: cap });

describe("deriveRuns", () => {
  it("keeps the spans of at least thirty calendar days, the ongoing one included, newest first", () => {
    const b = bot(100, { config_switches: [sw(0, "a"), sw(45, "b"), sw(95, "a")] });
    expect(deriveRuns(b).map((r) => [r.template_name, r.days, r.end == null])).toEqual([
      ["b", 50, false],
      ["a", 45, false],
    ]);
    // The ongoing span is four days old: not a run yet. Given more time it is.
    const later = deriveRuns(b, 130 * DAY);
    expect(later.map((r) => r.template_name)).toEqual(["a", "b", "a"]);
    expect(later[0]!.end).toBeNull();
  });

  it("re-bases each run to the close before its switch", () => {
    const b = bot(100, { config_switches: [sw(0, "a"), sw(45, "b")] });
    const [runB, runA] = deriveRuns(b);
    const base = b.points[45]!.index;
    expect(runB!.points[0]!.ts).toBe(46 * DAY);
    expect(runB!.points[0]!.return_pct).toBeCloseTo((b.points[46]!.index / base - 1) * 100, 6);
    expect(runA!.return_pct).toBeCloseTo((b.points[45]!.index / b.points[0]!.index - 1) * 100, 6);
    expect(runB!.cap_usdt).toBe(1000);
  });

  it("splits a span at a capital reset and starts the second part from the reset day", () => {
    const b = bot(100, {
      config_switches: [sw(0, "a", 1000), sw(70, "b")],
      capital_resets: [{ ts: 30 * DAY, cap_usdt: 500 }],
    });
    for (const p of b.points) if (p.ts >= 30 * DAY) p.index = Number((100 * 1.01 ** (p.ts / DAY - 30)).toFixed(4));
    // Before the reset: 29 full days, not a run. After it: 40 days at the new
    // capital, re-based to the reset day's own close. Then b: 28 days, not a run.
    const runs = deriveRuns(b);
    expect(runs.map((r) => [r.template_name, r.start / DAY, r.days, r.cap_usdt])).toEqual([["a", 30, 40, 500]]);
    expect(runs[0]!.points[0]!.return_pct).toBe(0);
  });

  it("is not a run when the base is a wiped-out account", () => {
    const b = bot(100, { config_switches: [sw(10, "a")] });
    for (const p of b.points) if (p.ts < 11 * DAY) p.index = 0;
    expect(deriveRuns(b)).toEqual([]);
  });

  it("caps a config at the five newest runs across bots", () => {
    const switches = Array.from({ length: 8 }, (_, i) => sw(i * 31, "a"));
    const one = bot(260, { id: "one", config_switches: switches });
    const two = bot(260, { id: "two", config_switches: [sw(0, "a"), sw(200, "b")] });
    const runs = runsForTemplate([one, two], "a");
    expect(runs).toHaveLength(MAX_RUNS_PER_TEMPLATE);
    expect(runs[0]!.start).toBeGreaterThan(runs[4]!.start);
    expect(runsForTemplate([one, two], "b")).toHaveLength(1);
  });
});

describe("selectWindow periods", () => {
  it("names the config active when the window opens and clips periods to the window", () => {
    const b = bot(200, { config_switches: [sw(0, "a"), sw(150, "b")] });
    const win = selectWindow(asSeries(b), 1); // 90D
    expect(win.kind).toBe("ok");
    if (win.kind !== "ok") return;
    const first = win.view[0]!.ts;
    expect(win.periods.map((p) => [p.template_name, p.start === first, p.startsInside])).toEqual([
      ["a", true, false],
      ["b", false, true],
    ]);
    expect(win.periods[0]!.end).toBe(win.periods[1]!.start);
    expect(win.periods[1]!.end).toBe(win.view[win.view.length - 1]!.ts);
    // Only the switch inside the window gets a dot.
    expect(win.switches.map((s) => s.template_name)).toEqual(["b"]);
  });

  it("starts the periods at the re-funding when the window holds one", () => {
    const b = bot(200, {
      config_switches: [sw(0, "a")],
      capital_resets: [{ ts: 150 * DAY, cap_usdt: 500 }],
    });
    const win = selectWindow(asSeries(b), 1);
    if (win.kind !== "ok") throw new Error(win.reason);
    expect(win.view[0]!.ts).toBe(150 * DAY);
    expect(win.periods).toEqual([{ start: 150 * DAY, end: 199 * DAY, template_name: "a", startsInside: false }]);
  });
});

describe("fmtCap", () => {
  it("prints the rounded capital", () => {
    expect([fmtCap(500), fmtCap(1000), fmtCap(2000), fmtCap(20000), fmtCap(1500), fmtCap(0)]).toEqual([
      "$500",
      "$1k",
      "$2k",
      "$20k",
      "$1.5k",
      "—",
    ]);
  });
});
