"""A description says how a template orders; a style with no wording is refused.

    python -m unittest scripts/ops/tests/test_describe_templates.py
"""
from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from describe_templates import STYLES_ZH, unworded_style  # noqa: E402


class UnwordedStyle(unittest.TestCase):
    def test_every_style_the_table_words_is_accepted(self):
        for style in STYLES_ZH:
            self.assertIsNone(unworded_style({"style": style}), style)

    def test_a_style_with_no_wording_is_refused_by_name(self):
        # The lab maps live.strategy_kind into these keys, so a kind added
        # upstream arrives here without a wording. The run must name it rather
        # than compose a description that never says how the template orders.
        reason = unworded_style({"style": "hydra"})
        self.assertIsNotNone(reason)
        self.assertIn("hydra", reason)

    def test_a_template_without_a_style_is_refused_too(self):
        self.assertIsNotNone(unworded_style({}))


if __name__ == "__main__":
    unittest.main()
