"""A template page's curve must show the drawdown its metrics state (issue #209).

    python -m unittest scripts/ops/tests/test_curve_points.py
"""
from __future__ import annotations

import math
import random
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from backtest_templates import MAX_POINTS, downsample  # noqa: E402


def worst_drawdown(equity: list[float]) -> float:
    peak, worst = equity[0], 0.0
    for value in equity:
        peak = max(peak, value)
        worst = max(worst, 1 - value / peak)
    return worst


def walk(n: int, seed: int) -> list[tuple]:
    rng = random.Random(seed)
    equity, rows = 10000.0, []
    for i in range(n):
        equity *= math.exp(rng.gauss(0.0002, 0.01))
        rows.append((i * 3600, 10000.0, equity))
    return rows


class CurvePoints(unittest.TestCase):
    def test_a_short_run_keeps_every_row(self):
        rows = walk(MAX_POINTS, 1)
        self.assertEqual(downsample(rows), rows)

    def test_a_long_run_fits_the_point_budget(self):
        points = downsample(walk(4000, 2))
        self.assertLessEqual(len(points), MAX_POINTS)
        self.assertGreater(len(points), MAX_POINTS - 10)

    def test_the_curve_keeps_the_runs_worst_drawdown(self):
        for seed in range(20):
            rows = walk(4000, seed)
            points = downsample(rows)
            self.assertAlmostEqual(worst_drawdown([r[2] for r in points]), worst_drawdown([r[2] for r in rows]),
                                   places=12, msg=f"seed {seed}")

    def test_a_trough_between_even_samples_stays_on_the_curve(self):
        rows = [(i, 100.0, 100.0) for i in range(3001)]
        rows[1501] = (1501, 100.0, 60.0)
        self.assertIn(rows[1501], downsample(rows))

    def test_the_first_and_last_rows_are_the_runs_own(self):
        # The chart rebases every period on the first point inside it and ends on the last.
        rows = walk(4000, 3)
        points = downsample(rows)
        self.assertEqual(points[0], rows[0])
        self.assertEqual(points[-1], rows[-1])

    def test_points_stay_in_time_order(self):
        points = downsample(walk(4000, 4))
        self.assertEqual([p[0] for p in points], sorted(p[0] for p in points))


if __name__ == "__main__":
    unittest.main()
