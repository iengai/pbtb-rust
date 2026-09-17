"""The cold-start gate: what passes a window, and when a run on record stands.

    python -m unittest scripts/ops/tests/test_stress_gate.py
"""
from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
import stress_gate  # noqa: E402

FEB26 = next(w for w in stress_gate.WINDOWS if w.label == "FEB26")
CONFIG = {"bot": {"long": {"n_positions": 5}}, "live": {}, "backtest": {"starting_balance": 1000}}


def record(config: dict, capital: int, passed: bool = True) -> dict:
    return {
        "capital": capital,
        "params_sha": stress_gate.bt.params_sha(config),
        "passed": passed,
        "windows": {w.label: {"start": w.start, "end": w.end, "drawdown_cap": w.drawdown_cap}
                    for w in stress_gate.WINDOWS},
    }


class Failure(unittest.TestCase):
    def test_a_completed_window_inside_its_cap_passes(self):
        metrics = {"backtest_completion_ratio": 1.0, "drawdown_worst": 0.319, "gain": 0.93}
        self.assertIsNone(stress_gate.failure(FEB26, metrics))

    def test_a_drawdown_past_the_cap_fails_whatever_the_gain(self):
        metrics = {"backtest_completion_ratio": 1.0, "drawdown_worst": 0.615, "gain": 1.4}
        self.assertIn("61.5%", stress_gate.failure(FEB26, metrics))

    def test_a_liquidation_fails(self):
        metrics = {"backtest_completion_ratio": 0.4, "drawdown_worst": 0.2}
        self.assertIn("liquidated", stress_gate.failure(FEB26, metrics))

    def test_a_run_without_the_metrics_fails(self):
        self.assertIsNotNone(stress_gate.failure(FEB26, {}))


class WindowConfig(unittest.TestCase):
    def test_the_window_starts_fresh_from_the_capital_and_leaves_the_input_alone(self):
        out = stress_gate.window_config(CONFIG, FEB26, 500)
        self.assertEqual((out["backtest"]["start_date"], out["backtest"]["end_date"],
                          out["backtest"]["starting_balance"]), (FEB26.start, FEB26.end, 500))
        self.assertEqual(out["bot"], CONFIG["bot"])
        self.assertEqual(CONFIG["backtest"], {"starting_balance": 1000})


class Covers(unittest.TestCase):
    def test_a_passed_run_of_these_parameters_at_this_capital_stands(self):
        self.assertTrue(stress_gate.covers(record(CONFIG, 700), CONFIG, 700))

    def test_another_capital_other_parameters_a_failed_run_or_none_do_not(self):
        other = {**CONFIG, "bot": {"long": {"n_positions": 9}}}
        self.assertFalse(stress_gate.covers(record(CONFIG, 700), CONFIG, 500))
        self.assertFalse(stress_gate.covers(record(other, 700), CONFIG, 700))
        self.assertFalse(stress_gate.covers(record(CONFIG, 700, passed=False), CONFIG, 700))
        self.assertFalse(stress_gate.covers(None, CONFIG, 700))

    def test_a_run_over_other_windows_does_not(self):
        old = record(CONFIG, 700)
        del old["windows"]["FEB26"]
        self.assertFalse(stress_gate.covers(old, CONFIG, 700))


if __name__ == "__main__":
    unittest.main()
