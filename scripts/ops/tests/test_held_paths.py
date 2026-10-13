"""The fix agent holds a merge tier only for the hooks and settings that refuse an agent's commands.

    python -m unittest scripts/ops/tests/test_held_paths.py
"""
from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import fix_issue  # noqa: E402

MERGE = fix_issue.MERGE_TIERS[0]


class HeldPathsTest(unittest.TestCase):
    def test_a_hook_holds_the_merge_tier(self):
        tier, note = fix_issue.held_for(MERGE, [".claude/hooks/guard-shell.py"])
        self.assertEqual(tier, fix_issue.HELD_TIER[MERGE])
        self.assertIn(".claude/hooks/guard-shell.py", note)

    def test_the_settings_hold_the_merge_tier(self):
        self.assertEqual(fix_issue.held_for(MERGE, [".claude/settings.local.json"])[0], fix_issue.HELD_TIER[MERGE])

    def test_skills_agents_and_docs_keep_the_merge_tier(self):
        staged = [".claude/skills/pbtb-ship/SKILL.md", ".claude/agents/pr-reviewer.md", "docs/governance.md"]
        self.assertEqual(fix_issue.held_for(MERGE, staged), (MERGE, ""))


if __name__ == "__main__":
    unittest.main()
