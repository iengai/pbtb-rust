import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { metricRows, sortTemplates, tradedLabels } from "./metrics";

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
  it("reads a share the run barely traded as <1%, not 0%", () => {
    // The profile lists every coin a fill went to, so a rounded `0%` would
    // say the run never touched a coin it did.
    expect(
      tradedLabels([
        { coin: "DOGE", share: 99.5 },
        { coin: "BTC", share: 0.5 },
        { coin: "ETH", share: 0.3 },
      ]),
    ).toEqual(["DOGE 100%", "BTC 1%", "ETH <1%"]);
  });
});
