"""A tuning is one template: the transfer finds the listed templates that
already trade a config's parameters.

    python -m unittest scripts/ops/tests/test_same_params.py
"""
from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
import backtest_templates  # noqa: E402
import transfer_config_to_s3  # noqa: E402


def config(balance: int, exposure: float = 1.0) -> dict:
    return {
        "bot": {"long": {"total_wallet_exposure_limit": exposure}},
        "live": {"leverage": 10},
        "backtest": {"starting_balance": balance},
    }


class TradingSha(unittest.TestCase):
    def test_the_candle_directory_is_not_the_strategy(self):
        padded = {**config(500), "backtest": {"starting_balance": 500, "ohlcv_source_dir": "caches/ohlcv_padded"}}
        combined = {**config(500), "backtest": {"starting_balance": 500, "ohlcv_source_dir": "caches/ohlcv_combined"}}
        self.assertEqual(backtest_templates.trading_sha(padded), backtest_templates.trading_sha(combined))
        self.assertEqual(backtest_templates.trading_sha(padded), backtest_templates.trading_sha(config(500)))
        self.assertNotEqual(backtest_templates.trading_sha(config(500)), backtest_templates.trading_sha(config(1000)))


class SameParams(unittest.TestCase):
    def test_the_backtest_block_and_our_blocks_do_not_tell_tunings_apart(self):
        at_500 = {**config(500), "pbtb": {"name": "tpl-a"}, "lab": {"iter": 1}}
        self.assertEqual(backtest_templates.params_sha(at_500), backtest_templates.params_sha(config(1000)))
        self.assertNotEqual(backtest_templates.params_sha(config(500)),
                            backtest_templates.params_sha(config(500, exposure=2.0)))

    def test_finds_the_other_templates_that_carry_the_parameters(self):
        index = [
            {"name": "tpl-a", "params_sha": backtest_templates.params_sha(config(500))},
            {"name": "tpl-b", "params_sha": backtest_templates.params_sha(config(500, exposure=2.0))},
            {"name": "tpl-old"},
        ]
        twins = transfer_config_to_s3.same_params(config(1000), "tpl-new", index)
        self.assertEqual([row["name"] for row in twins], ["tpl-a"])

    def test_overwriting_a_template_that_carries_the_parameters_finds_none(self):
        sha = backtest_templates.params_sha(config(500))
        index = [{"name": "tpl-a", "params_sha": sha}, {"name": "tpl-b", "params_sha": sha}]
        self.assertEqual(transfer_config_to_s3.same_params(config(500), "tpl-a", index), [])

    def test_overwriting_a_template_with_another_one_s_parameters_finds_it(self):
        index = [
            {"name": "tpl-a", "params_sha": backtest_templates.params_sha(config(500, exposure=2.0))},
            {"name": "tpl-b", "params_sha": backtest_templates.params_sha(config(500))},
        ]
        twins = transfer_config_to_s3.same_params(config(700), "tpl-a", index)
        self.assertEqual([row["name"] for row in twins], ["tpl-b"])


if __name__ == "__main__":
    unittest.main()
