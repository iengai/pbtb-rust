"""A description's stress windows are the lab's runs at one capital: a template
saying another capital is refused.

    python -m unittest scripts/ops/tests/test_stress_capital.py
"""
from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
import describe_templates  # noqa: E402


class StressCapital(unittest.TestCase):
    def test_windows_run_at_the_template_s_capital_describe_it(self):
        self.assertIsNone(describe_templates.stale_windows({"capital_usdt": 700}, {"capital": 700}))

    def test_a_template_at_another_capital_is_refused_with_both_named(self):
        why = describe_templates.stale_windows({"capital_usdt": 500}, {"capital": 700})
        self.assertIn("$700", why)
        self.assertIn("$500", why)

    def test_a_template_with_no_capital_is_refused(self):
        self.assertIsNotNone(describe_templates.stale_windows({}, {"capital": 700}))

    def test_every_entry_names_the_capital_its_windows_were_run_at(self):
        for tid, entry in describe_templates.PUBLIC.items():
            self.assertIsInstance(entry.get("capital"), int, tid)


if __name__ == "__main__":
    unittest.main()
