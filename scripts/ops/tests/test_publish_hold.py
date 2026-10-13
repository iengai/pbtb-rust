"""The fix agent's publish holds a merge tier only for the guards and the gate.

    python -m unittest scripts/ops/tests/test_publish_hold.py
"""
from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import fix_issue  # noqa: E402

MERGE = fix_issue.MERGE_TIERS[0]
OPEN = fix_issue.PR_TIERS[0]


class PublishHoldTest(unittest.TestCase):
    def test_docs_skills_and_agents_merge_at_the_granted_tier(self):
        staged = ["docs/governance.md", ".claude/skills/pbtb-ship/SKILL.md", ".claude/agents/pr-reviewer.md",
                  "AGENTS.md", "REVIEW.md"]
        self.assertEqual(fix_issue.held_for(MERGE, staged), (MERGE, ""))

    def test_a_hook_holds_the_tier(self):
        tier, note = fix_issue.held_for(MERGE, ["src/main.rs", ".claude/hooks/guard-shell.py"])
        self.assertEqual(tier, fix_issue.HELD_TIER[MERGE])
        self.assertIn(".claude/hooks/guard-shell.py", note)

    def test_the_tracked_and_local_settings_hold_the_tier(self):
        for f in (".claude/settings.json", ".claude/settings.local.json"):
            self.assertEqual(fix_issue.held_for(MERGE, [f])[0], fix_issue.HELD_TIER[MERGE], f)

    def test_the_gate_script_holds_the_tier(self):
        self.assertEqual(fix_issue.held_for(MERGE, [".claude/skills/verify/scripts/gate.sh"])[0],
                         fix_issue.HELD_TIER[MERGE])

    def test_a_tier_below_merge_is_left_alone(self):
        self.assertEqual(fix_issue.held_for(OPEN, [".claude/hooks/guard-shell.py"]), (OPEN, ""))


if __name__ == "__main__":
    unittest.main()
