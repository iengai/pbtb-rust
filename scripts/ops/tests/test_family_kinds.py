"""A dry-run shadow family is not an engine line, and the audit must not call it drift.

    python -m unittest scripts/ops/tests/test_family_kinds.py
"""
from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import pbtb_ops  # noqa: E402

PREFIX = "scalable-cluster-dev-passivbot"


class FamilyKindTest(unittest.TestCase):
    def test_engine_lines_map_to_their_keys(self):
        self.assertEqual(pbtb_ops.family_engine(PREFIX, PREFIX), "7")
        self.assertEqual(pbtb_ops.family_engine(f"{PREFIX}-v8", PREFIX), "8")
        self.assertEqual(pbtb_ops.family_engine(f"{PREFIX}-v8-rs", PREFIX), "8rs")
        self.assertFalse(pbtb_ops.is_shadow_family(f"{PREFIX}-v8-rs", PREFIX))

    def test_a_shadow_is_known_and_is_not_an_engine_line(self):
        family = f"{PREFIX}-v8-rs-shadow-paper2"
        self.assertIsNone(pbtb_ops.family_engine(family, PREFIX))
        self.assertTrue(pbtb_ops.is_shadow_family(family, PREFIX))

    def test_an_unknown_suffix_is_neither(self):
        family = f"{PREFIX}-v9-py"
        self.assertIsNone(pbtb_ops.family_engine(family, PREFIX))
        self.assertFalse(pbtb_ops.is_shadow_family(family, PREFIX))


if __name__ == "__main__":
    unittest.main()
