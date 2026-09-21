"""A description says how a template orders; a style with no wording is refused.

    python -m unittest scripts/ops/tests/test_describe_templates.py
"""
from __future__ import annotations

import json
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from describe_templates import PUBLIC, SITE_DIR, unworded_style  # noqa: E402


class UnwordedStyle(unittest.TestCase):
    def test_every_listed_template_has_a_style_the_table_words(self):
        # The gate goes red when a template lands on a style with no Chinese
        # wording, rather than the operator finding out from an --apply run
        # that exits 1 with the template left undescribed.
        for tid in PUBLIC:
            path = SITE_DIR / f"{tid}.json"
            if not path.exists():
                continue
            style = json.loads(path.read_text(encoding="utf-8")).get("style")
            self.assertIsNone(unworded_style({"style": style}), f"{tid}: {style}")

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
