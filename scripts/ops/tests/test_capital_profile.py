"""A template's capital is the least it is offered for; the capital profile shows
what a larger balance does with the same parameters.

    python -m unittest scripts/ops/tests/test_capital_profile.py
"""
from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from backtest_templates import capital_drawdown, fill_shares, ladder_for, wants_profile  # noqa: E402


def fills(path: Path, coins: list[str]) -> Path:
    (path / "fills.csv").write_text("index,coin,qty\n" + "".join(f"{i},{c},1\n" for i, c in enumerate(coins)),
                                    encoding="utf-8")
    return path


class CapitalProfile(unittest.TestCase):
    def test_the_ladder_starts_at_the_templates_own_balance(self):
        self.assertEqual(ladder_for(300), [300.0, 500.0, 700.0, 1000.0, 1500.0, 2000.0, 3000.0, 5000.0, 10000.0])
        self.assertEqual(ladder_for(100)[:2], [100.0, 300.0])
        self.assertEqual(ladder_for(2500), [2500.0, 3000.0, 5000.0, 10000.0])
        self.assertEqual(ladder_for(10000), [10000.0])

    def test_fill_shares_are_percentages_of_the_fill_count_largest_first(self):
        with tempfile.TemporaryDirectory() as tmp:
            shares = fill_shares(fills(Path(tmp), ["DOGE"] * 6 + ["XRP"] * 3 + ["ADA"]))
        self.assertEqual(shares, [{"coin": "DOGE", "share": 60.0}, {"coin": "XRP", "share": 30.0},
                                  {"coin": "ADA", "share": 10.0}])

    def test_a_coin_under_one_percent_of_the_fills_is_left_out(self):
        with tempfile.TemporaryDirectory() as tmp:
            shares = fill_shares(fills(Path(tmp), ["DOGE"] * 199 + ["BTC"]))
        self.assertEqual([s["coin"] for s in shares], ["DOGE"])

    def test_a_run_without_fills_has_no_shares(self):
        with tempfile.TemporaryDirectory() as tmp:
            self.assertEqual(fill_shares(Path(tmp)), [])
            self.assertEqual(fill_shares(fills(Path(tmp), [])), [])

    def test_a_share_is_compared_with_the_floor_before_it_is_rounded(self):
        with tempfile.TemporaryDirectory() as tmp:
            shares = fill_shares(fills(Path(tmp), ["DOGE"] * 1039 + ["BTC"] * 10))  # BTC 0.953%
        self.assertEqual([s["coin"] for s in shares], ["DOGE"])

    def test_a_template_is_profiled_when_asked_or_when_it_already_has_a_profile(self):
        # (flag, force, had, kept)
        self.assertTrue(wants_profile(True, False, False, False))    # asked, none yet
        self.assertFalse(wants_profile(True, False, True, True))     # asked, current: kept
        self.assertTrue(wants_profile(False, False, True, False))    # a stale one (engine, window, candles) runs again
        self.assertTrue(wants_profile(False, True, True, True))      # --force reruns what the artifact carries
        self.assertFalse(wants_profile(False, True, False, False))   # --force alone starts none
        self.assertFalse(wants_profile(False, False, False, False))

    def test_the_list_reads_the_median_and_the_worst_drawdown(self):
        profile = {"rows": [{"drawdown_worst": v} for v in (0.20, 0.50, 0.19, None, 0.21)]}
        read = capital_drawdown(profile)
        self.assertAlmostEqual(read["median"], 0.205)
        self.assertEqual(read["worst"], 0.50)
        self.assertIsNone(capital_drawdown(None))
        self.assertIsNone(capital_drawdown({"rows": []}))


if __name__ == "__main__":
    unittest.main()
