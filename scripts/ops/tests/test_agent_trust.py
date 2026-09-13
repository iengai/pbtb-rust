"""The fix agent trusts the local agent App by its numeric user id, never by a login.

    python -m unittest scripts/ops/tests/test_agent_trust.py
"""
from __future__ import annotations

import os
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import fix_issue  # noqa: E402

MERGE = fix_issue.MERGE_TIERS[0]
BODY = f"### Problem\n\nx\n\n### How far the agent may go\n\n{MERGE}\n"
AGENT_ID = 4242


def comment(user_id: int, login: str, association: str, body: str) -> dict:
    return {"user": {"id": user_id, "login": login}, "author_association": association, "body": body}


class TrustTest(unittest.TestCase):
    def setUp(self):
        self._env = {k: os.environ.get(k) for k in ("LOCAL_AGENT_USER_ID", "GH_REPO")}
        os.environ["LOCAL_AGENT_USER_ID"] = str(AGENT_ID)
        os.environ["GH_REPO"] = "owner/repo"

    def tearDown(self):
        for k, v in self._env.items():
            if v is None:
                os.environ.pop(k, None)
            else:
                os.environ[k] = v

    def tier(self, association: str, user_id: int) -> str:
        return fix_issue.resolve_tier(1, BODY, author=lambda repo, issue: (association, user_id))[0]

    def test_merge_tier_stands_for_the_local_agent(self):
        self.assertEqual(self.tier("CONTRIBUTOR", AGENT_ID), MERGE)

    def test_merge_tier_stands_for_a_member(self):
        self.assertEqual(self.tier("MEMBER", 1), MERGE)

    def test_merge_tier_is_held_for_another_bot(self):
        self.assertEqual(self.tier("CONTRIBUTOR", 41898282), fix_issue.HELD_TIER[MERGE])

    def test_merge_tier_is_held_when_the_variable_is_unset(self):
        os.environ["LOCAL_AGENT_USER_ID"] = ""
        self.assertEqual(self.tier("NONE", AGENT_ID), fix_issue.HELD_TIER[MERGE])

    def test_comments_are_kept_by_id_not_by_login(self):
        comments = [
            comment(AGENT_ID, "pbtb-local-agent[bot]", "NONE", "from the app"),
            comment(7, "pbtb-local-agent", "NONE", "from a lookalike user"),
            comment(1, "owner", "OWNER", "from the owner"),
        ]
        kept = [c["body"] for c in fix_issue.trusted_comments(comments)]
        self.assertEqual(kept, ["from the app", "from the owner"])


if __name__ == "__main__":
    unittest.main()
