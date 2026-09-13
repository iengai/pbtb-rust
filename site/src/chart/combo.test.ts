import { describe, expect, it } from "vitest";
import {
  type ComboSeries,
  DAY,
  MIN_SPAN_DAYS,
  PRESETS,
  clampSpan,
  domainOf,
  initialSelection,
  presetOf,
  presetSpan,
  rebase,
  valueAt,
} from "./combo";

// A series with one point per day from `start`, climbing 1% a day from 100.
function daily(id: string, start: number, days: number): ComboSeries {
  return {
    id,
    label: id,
    color: "#000",
    points: Array.from({ length: days }, (_, d) => ({ ts: start + d * DAY, v: 100 * 1.01 ** d })),
  };
}

const backtest = daily("bt", 0, 400);
const run = daily("run", 300 * DAY, 60); // ends at day 359, inside the backtest

describe("domain and presets", () => {
  it("spans the earliest to the latest point across series", () => {
    expect(domainOf([backtest, run])).toEqual({ from: 0, to: 399 * DAY });
    expect(domainOf([daily("late", 500 * DAY, 3), backtest])).toEqual({ from: 0, to: 502 * DAY });
  });

  it("has no domain without two points", () => {
    expect(domainOf([])).toBeNull();
    expect(domainOf([{ id: "one", label: "", color: "", points: [{ ts: 5, v: 1 }] }])).toBeNull();
  });

  it("anchors a preset at the latest point and lets All take the domain", () => {
    const domain = { from: 0, to: 399 * DAY };
    expect(presetSpan(domain, 30)).toEqual({ from: 369 * DAY, to: 399 * DAY });
    expect(presetSpan(domain, null)).toEqual(domain);
    expect(presetSpan({ from: 380 * DAY, to: 399 * DAY }, 30)).toEqual({ from: 380 * DAY, to: 399 * DAY });
  });

  it("names the preset a span is, and none for a brushed span", () => {
    const domain = { from: 0, to: 399 * DAY };
    expect(PRESETS[presetOf(domain, presetSpan(domain, 90))]!.k).toBe("3M");
    expect(PRESETS[presetOf(domain, domain)]!.k).toBe("All");
    expect(presetOf(domain, { from: 10 * DAY, to: 200 * DAY })).toBe(-1);
  });

  it("names a domain shorter than every preset All", () => {
    const short = { from: 0, to: 20 * DAY };
    expect(PRESETS[presetOf(short, presetSpan(short, 30))]!.k).toBe("All");
  });
});

describe("clampSpan", () => {
  const domain = { from: 100 * DAY, to: 200 * DAY };

  it("keeps a span inside the domain and in order", () => {
    expect(clampSpan({ from: 50 * DAY, to: 150 * DAY }, domain)).toEqual({ from: 100 * DAY, to: 150 * DAY });
    expect(clampSpan({ from: 150 * DAY, to: 120 * DAY }, domain)).toEqual({ from: 120 * DAY, to: 150 * DAY });
  });

  it("never returns a span shorter than the minimum", () => {
    const min = MIN_SPAN_DAYS * DAY;
    expect(clampSpan({ from: 150 * DAY, to: 150 * DAY + 10 }, domain)).toEqual({
      from: 150 * DAY + 10 - min,
      to: 150 * DAY + 10,
    });
    // Pinned at the domain's start it grows to the right instead.
    expect(clampSpan({ from: 100 * DAY, to: 100 * DAY }, domain)).toEqual({ from: 100 * DAY, to: 100 * DAY + min });
  });
});

describe("rebase", () => {
  it("re-bases each series at its first point inside the span", () => {
    const span = { from: 300 * DAY, to: 399 * DAY };
    const curves = rebase([backtest, run], span);
    expect(curves.map((c) => c.series.id)).toEqual(["bt", "run"]);
    for (const c of curves) {
      expect(c.view[0]!.ts).toBe(300 * DAY);
      expect(c.view[0]!.pct).toBe(0);
    }
    // The same 1%/day path from the same start reads the same on both.
    expect(curves[0]!.view[59]!.pct).toBeCloseTo(curves[1]!.view[59]!.pct, 6);
    expect(curves[1]!.view).toHaveLength(60);
  });

  it("starts a run at 0% where it started when the span opens earlier", () => {
    const [, r] = rebase([backtest, run], { from: 200 * DAY, to: 399 * DAY });
    expect(r!.series.id).toBe("run");
    expect(r!.view[0]).toEqual({ ts: 300 * DAY, pct: 0 });
  });

  it("leaves out a series with fewer than two points inside, or a wiped one", () => {
    expect(rebase([backtest, run], { from: 0, to: 100 * DAY }).map((c) => c.series.id)).toEqual(["bt"]);
    expect(rebase([run], { from: 359 * DAY, to: 399 * DAY })).toEqual([]);
    const dead: ComboSeries = { id: "dead", label: "", color: "", points: [{ ts: 0, v: 0 }, { ts: DAY, v: 0 }] };
    expect(rebase([dead], { from: 0, to: DAY })).toEqual([]);
  });
});

describe("valueAt", () => {
  const [c] = rebase([daily("s", 0, 3)], { from: 0, to: 2 * DAY });
  it("interpolates inside the curve and is null outside it", () => {
    expect(valueAt(c!.view, DAY / 2)).toBeCloseTo(0.5, 6);
    expect(valueAt(c!.view, 2 * DAY)).toBeCloseTo(2.01, 6);
    expect(valueAt(c!.view, -1)).toBeNull();
    expect(valueAt(c!.view, 3 * DAY)).toBeNull();
  });
});

describe("initialSelection", () => {
  it("is the newest run only, or nothing", () => {
    expect(initialSelection([{ key: "b:9" }, { key: "a:3" }])).toEqual(new Set(["b:9"]));
    expect(initialSelection([])).toEqual(new Set());
  });
});
