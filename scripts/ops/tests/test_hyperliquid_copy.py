"""A Bybit template's Hyperliquid copy: its backtest block, the gate that stands
for it, the refusal it passes, and what its artifact and description say.

    python -m unittest scripts/ops/tests/test_hyperliquid_copy.py
"""
from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
import backtest_templates  # noqa: E402
import describe_templates  # noqa: E402
import hyperliquid_copy  # noqa: E402
import stress_gate  # noqa: E402
import transfer_config_to_s3  # noqa: E402

BYBIT = {
    "config_version": "v8.1.0",
    "bot": {"long": {"risk": {"total_wallet_exposure_limit": 1.5, "n_positions": 3}}},
    "live": {"approved_coins": {"long": ["BTC", "APT"], "short": ["BTC", "APT"]}},
    "backtest": {
        "exchanges": ["bybit"], "ohlcv_source_dir": "caches/ohlcv_combined", "candle_interval_minutes": 1,
        "start_date": "2024-03-15", "end_date": "2026-09-11", "starting_balance": 10000,
        "maker_fee_override": 0.0004,
        "market_settings": {"overrides": {"BTC": {"min_cost": 10}, "APT": {"min_cost": 12}}},
    },
    "pbtb": {"name": "tpl-bybit001", "exchange": "bybit"},
    "lab": {"iter": 22},
}


def copy() -> tuple[dict, dict]:
    return hyperliquid_copy.hyperliquid_copy(BYBIT, "2026-03-30", "2026-09-22")


class Copy(unittest.TestCase):
    def test_only_the_backtest_block_moves_to_hyperliquid_candles(self):
        config, lab = copy()
        self.assertNotIn("pbtb", config)
        self.assertEqual((config["bot"], config["live"]), (BYBIT["bot"], BYBIT["live"]))
        backtest = config["backtest"]
        self.assertEqual((backtest["exchanges"], backtest["ohlcv_source_dir"], backtest["candle_interval_minutes"]),
                         (["hyperliquid"], "caches/ohlcv_hl", 60))
        self.assertEqual((backtest["start_date"], backtest["end_date"]), ("2026-03-30", "2026-09-22"))
        self.assertEqual((backtest["maker_fee_override"], backtest["starting_balance"]), (0.0004, 10000))
        self.assertEqual(lab, {"iter": 22, "copied_from": "tpl-bybit001"})
        self.assertEqual(BYBIT["backtest"]["exchanges"], ["bybit"])

    def test_every_coin_keeps_at_least_the_hyperliquid_order_floor(self):
        overrides = copy()[0]["backtest"]["market_settings"]["overrides"]
        self.assertEqual(overrides, {"BTC": {"min_cost": 10}, "APT": {"min_cost": 12}})

    def test_a_hyperliquid_template_is_not_copied_again(self):
        with self.assertRaises(ValueError):
            hyperliquid_copy.hyperliquid_copy({**BYBIT, "pbtb": {"exchange": "hyperliquid"}}, "a", "b")

    def test_the_window_is_the_days_every_coin_has_less_the_warm_up(self):
        days = {"BTC": ["2026-02-11", "2026-09-22"], "APT": ["2026-02-28", "2026-09-23"]}
        self.assertEqual(hyperliquid_copy.window(days), ("2026-03-30", "2026-09-22"))
        with self.assertRaises(ValueError):
            hyperliquid_copy.window({"BTC": ["2026-02-11", "2026-09-22"], "APT": []})

    def test_shard_days_reads_the_coin_directory(self):
        with tempfile.TemporaryDirectory() as root:
            shards = Path(root) / "hyperliquid" / "1m" / "APT_USDC_USDC"
            shards.mkdir(parents=True)
            for day in ("2026-03-02", "2026-03-01"):
                (shards / f"{day}.npy").touch()
            self.assertEqual(hyperliquid_copy.shard_days(Path(root), "APT"), ["2026-03-01", "2026-03-02"])


class Gate(unittest.TestCase):
    def test_the_windows_of_a_copy_run_on_bybit_candles_so_the_bybit_run_stands(self):
        config, _ = copy()
        backtest = stress_gate.stress_config(config)["backtest"]
        self.assertEqual((backtest["exchanges"], backtest["ohlcv_source_dir"], backtest["candle_interval_minutes"]),
                         (["bybit"], "caches/ohlcv_combined", 1))
        record = {
            "capital": 10000, "passed": True, "params_sha": backtest_templates.params_sha(BYBIT),
            "engine": "v8.1.0", "ohlcv_source_dir": "caches/ohlcv_combined",
            "windows": {w.label: {"start": w.start, "end": w.end, "drawdown_cap": w.drawdown_cap}
                        for w in stress_gate.WINDOWS},
        }
        self.assertTrue(stress_gate.covers(record, config, 10000))

    def test_a_bybit_config_runs_as_it_is(self):
        self.assertIs(stress_gate.stress_config(BYBIT), BYBIT)


class SameParams(unittest.TestCase):
    def test_a_tuning_is_one_template_per_exchange(self):
        config, _ = copy()
        index = [{"name": "tpl-bybit001", "params_sha": backtest_templates.params_sha(BYBIT)}]
        self.assertEqual(transfer_config_to_s3.same_params(config, "tpl-new", index, "hyperliquid"), [])
        self.assertEqual([r["name"] for r in transfer_config_to_s3.same_params(config, "tpl-new", index)],
                         ["tpl-bybit001"])


class Artifact(unittest.TestCase):
    def template(self) -> backtest_templates.Template:
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / "tpl-hl.json"
            path.write_text(backtest_templates.json.dumps(copy()[0]), encoding="utf-8")
            return backtest_templates.Template(path, end_date="2026-09-22")

    def test_a_run_one_candle_short_is_complete_and_a_shorter_one_is_not(self):
        template = self.template()
        one_short = 1 - 1 / (176 * 24)
        self.assertEqual(backtest_templates.completed({"backtest_completion_ratio": one_short}, template),
                         {"backtest_completion_ratio": 1.0})
        self.assertEqual(backtest_templates.completed({"backtest_completion_ratio": 0.99}, template),
                         {"backtest_completion_ratio": 0.99})

    def test_a_minute_run_one_candle_short_is_not_rounded(self):
        with tempfile.TemporaryDirectory() as root:
            path = Path(root) / "tpl-by.json"
            path.write_text(backtest_templates.json.dumps(BYBIT), encoding="utf-8")
            template = backtest_templates.Template(path, end_date="2026-09-11")
        metrics = {"backtest_completion_ratio": 0.99999}
        self.assertEqual(backtest_templates.completed(metrics, template), metrics)


class Describe(unittest.TestCase):
    ARTIFACT = {"exchange": "hyperliquid", "candle_minutes": 60, "start": "2026-03-30", "end": "2026-09-22",
                "coins": ["BTC", "APT"], "metrics": {"gain": 1.137, "drawdown_worst": 0.134,
                                                      "backtest_completion_ratio": 1.0}}
    BYBIT_ARTIFACT = {"start": "2024-03-15", "end": "2026-09-11",
                      "metrics": {"gain": 2.05, "drawdown_worst": 0.089, "backtest_completion_ratio": 1.0}}

    def test_the_copy_says_its_candles_and_quotes_the_bybit_run_and_the_check(self):
        config = {**copy()[0], "pbtb": {"universe": "mix24", "style": "martingale", "capital_usdt": 10000}}
        entry = {"capital": 10000, "character": "c", "bybit": "tpl-bybit001",
                 "stress": [describe_templates.window("W", 0.025, 0.026)],
                 "transfer": {"window": "2026-03-30～09-12", "bybit": (1.025, 0.265), "hyperliquid": (1.037, 0.134)}}
        text = describe_templates.describe(config, self.ARTIFACT, entry, self.BYBIT_ARTIFACT)
        self.assertIn("📈 回测 2026-03-30～2026-09-22（Hyperliquid 1 小时K线）：1.14 倍 · 📉 最大回撤 13.4%", text)
        self.assertIn("📈 回测 2024-03-15～2026-09-11（同参数，Bybit 1 分钟K线）：2.05 倍", text)
        self.assertIn("🔁 迁移检验 2026-03-30～09-12", text)
        self.assertIn("🧪 压力测试（单独起跑，Bybit K线）· W：+2.6% · 回撤 2.5%", text)

    def test_a_minute_backtest_reads_as_before(self):
        self.assertEqual(describe_templates.candles_of({"candle_minutes": 1}), "")
        self.assertEqual(describe_templates.candles_of({}), "")


if __name__ == "__main__":
    unittest.main()
