import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { metricRows, sortTemplates, tierOf, tradedLabels } from "./metrics";

// The committed backtests are the input the card is built from, so every one is
// checked: no row may render a raw float or a raw JSON literal.
const dir = fileURLToPath(new URL("../../templates", import.meta.url));
const files = readdirSync(dir).filter((f) => f.endsWith(".json"));

// "—", or a signed number with at most three decimals and an optional percent
// sign. A raw float, "null", "NaN" and "undefined" all fail this.
const FORMATTED = /^(—|[+-]?\d+(\.\d{1,3})?%?)$/;

type WithMetrics = { name?: string; metrics: Record<string, number> };

function templatesIn(file: string): WithMetrics[] {
  const parsed = JSON.parse(readFileSync(file, "utf8")) as WithMetrics | WithMetrics[];
  return Array.isArray(parsed) ? parsed : [parsed];
}

describe("metricRows", () => {
  it("renders every committed template's metrics as a formatted value", () => {
    expect(files.length).toBeGreaterThan(0);
    for (const f of files) {
      for (const template of templatesIn(join(dir, f))) {
        for (const row of metricRows(template.metrics)) {
          expect(row.value, `${template.name ?? f}: ${row.key}`).toMatch(FORMATTED);
        }
      }
    }
  });

  it("renders a metric with no value as — and rounds a key the catalog does not know", () => {
    expect(metricRows({ exposure_ratios_mean_long: null as unknown as number })).toEqual([
      { key: "exposure_ratios_mean_long", value: "—" },
    ]);
    expect(metricRows({ gain: Number.NaN })).toEqual([{ key: "gain", value: "—" }]);
    expect(metricRows({ future_metric: 1.23456789 })).toEqual([{ key: "future_metric", value: "1.235" }]);
  });
});

describe("sortTemplates", () => {
  const tpl = (name: string, starting_balance: number | null, gain: number, completion = 1) => ({
    name,
    starting_balance,
    metrics: { gain, drawdown_worst: 0.3, backtest_completion_ratio: completion },
  });
  const list = [tpl("a", 1000, 5), tpl("b", 500, 9), tpl("wiped", 100, 50, 0.4), tpl("c", 500, 12), tpl("old", null, 7)];
  const names = (sorted: { name: string }[]) => sorted.map((t) => t.name);

  it("sorts by the key's own direction, ties by gain", () => {
    expect(names(sortTemplates(list, "gain"))).toEqual(["c", "b", "old", "a", "wiped"]);
    expect(names(sortTemplates(list, "capital"))).toEqual(["c", "b", "a", "old", "wiped"]);
  });

  it("keeps a wiped-out template and one without the value last when flipped", () => {
    expect(names(sortTemplates(list, "capital", true))).toEqual(["a", "c", "b", "old", "wiped"]);
  });
});

describe("tradedLabels", () => {
  it("keeps a coin the run barely traded off 0%, however small its share", () => {
    // The profile lists every coin a fill went to, so a rounded `0%` would
    // say the run never touched a coin it did. A single fill in a long run
    // reaches the artifact as a share that rounds to 0.0.
    expect(
      tradedLabels([
        { coin: "DOGE", share: 98.9 },
        { coin: "BTC", share: 0.5 },
        { coin: "ETH", share: 0.3 },
        { coin: "SOL", share: 0 },
      ]),
    ).toEqual(["DOGE 99%", "BTC 1%", "ETH <1%", "SOL <1%"]);
  });

  it("does not give one coin the whole run while others are listed beside it", () => {
    expect(
      tradedLabels([
        { coin: "DOGE", share: 99.6 },
        { coin: "BTC", share: 0.4 },
      ]),
    ).toEqual(["DOGE >99%", "BTC <1%"]);
    expect(tradedLabels([{ coin: "DOGE", share: 100 }])).toEqual(["DOGE 100%"]);
  });
});

// A field a published file carries is one the README documents (docs/conventions.md).
describe("the site README", () => {
  const readme = readFileSync(fileURLToPath(new URL("../../README.md", import.meta.url)), "utf-8");
  it("names every top-level field of the committed artifacts and index", () => {
    const keys = new Set<string>();
    for (const f of files) {
      const data = JSON.parse(readFileSync(join(dir, f), "utf-8"));
      for (const row of Array.isArray(data) ? data : [data]) Object.keys(row).forEach((k) => keys.add(k));
    }
    const missing = [...keys].filter((k) => !new RegExp(`\\b${k}\\b`).test(readme));
    expect(missing).toEqual([]);
  });
});

describe("tierOf", () => {
  it("reads a drawdown against the lab's bands, each limit inclusive", () => {
    expect(tierOf(0.12)).toBe("guard");
    expect(tierOf(0.134)).toBe("steady");
    expect(tierOf(0.2)).toBe("steady");
    expect(tierOf(0.2007)).toBe("balanced");
    expect(tierOf(0.3007)).toBe("bold");
    expect(tierOf(0.41)).toBe("extreme");
  });
});
