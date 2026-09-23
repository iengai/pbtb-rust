"""A fix run that stops before the harness starts still leaves a result.json for publish.

    python -m unittest scripts/ops/tests/test_fix_finish.py
"""
from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import fix_issue  # noqa: E402


class FinishTest(unittest.TestCase):
    def test_not_started_writes_its_result_into_a_missing_out_dir(self):
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp) / "fix"
            result = {"issue": 1, "title": "t", "tier": "", "tier_note": "", "model": "?",
                      "fell_back": [], "trail": [], "verdict": "not_started"}
            self.assertEqual(fix_issue.finish(out, result, dry=False), 0)
            self.assertEqual(json.loads((out / "result.json").read_text(encoding="utf-8"))["verdict"],
                             "not_started")


if __name__ == "__main__":
    unittest.main()
