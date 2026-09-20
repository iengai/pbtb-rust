"""The catalogue's first split: a template holds one position at a time or several.

    python -m unittest scripts/ops/tests/test_position_class.py
"""
from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from backtest_templates import position_class  # noqa: E402


def v8(long_n: float, short_n: float = 0, short_exposure: float = 0.0) -> dict:
    return {"bot": {"long": {"risk": {"total_wallet_exposure_limit": 2.5, "n_positions": long_n}},
                    "short": {"risk": {"total_wallet_exposure_limit": short_exposure, "n_positions": short_n}}}}


class PositionClass(unittest.TestCase):
    def test_one_position_is_single_and_two_or_more_are_multi(self):
        self.assertEqual(position_class(v8(1.0)), "single")
        self.assertEqual(position_class(v8(2)), "multi")
        self.assertEqual(position_class(v8(7.0)), "multi")

    def test_a_fractional_count_rounds_as_passivbot_rounds_it(self):
        self.assertEqual(position_class(v8(1.6)), "multi")
        self.assertEqual(position_class(v8(1.4)), "single")
        self.assertIsNone(position_class(v8(0.4)))

    def test_a_side_without_exposure_does_not_count(self):
        self.assertEqual(position_class(v8(1, short_n=5, short_exposure=0.0)), "single")
        self.assertEqual(position_class(v8(1, short_n=5, short_exposure=1.0)), "multi")

    def test_the_flat_v7_shape_reads_the_same(self):
        flat = {"bot": {"long": {"total_wallet_exposure_limit": 1.0, "n_positions": 3.0},
                        "short": {"total_wallet_exposure_limit": 0.0, "n_positions": 0.0}}}
        self.assertEqual(position_class(flat), "multi")

    def test_a_side_holds_no_more_positions_than_it_has_coins(self):
        one_coin = {**v8(3), "live": {"approved_coins": {"long": ["XRP"], "short": ["XRP"]}}}
        self.assertEqual(position_class(one_coin), "single")
        self.assertEqual(position_class({**v8(3), "live": {"approved_coins": ["XRP", "BTC"]}}), "multi")

    def test_a_config_that_trades_no_side_has_no_class(self):
        self.assertIsNone(position_class({"bot": {}}))
        self.assertIsNone(position_class(v8(0)))


if __name__ == "__main__":
    unittest.main()
