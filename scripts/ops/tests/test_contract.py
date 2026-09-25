"""The `contract` check lets a local agent PR merge itself only on a current review and no hold.

    python -m unittest scripts/ops/tests/test_contract.py
"""
from __future__ import annotations

import os
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import contract  # noqa: E402
import fix_issue  # noqa: E402

AGENT, OWNER, OTHER = 4242, 1, 99
HEAD = "abcdef1234567890"
VERDICT = f"Review-verdict: pass @ {HEAD[:7]} — pr-reviewer 0 important, 2 nit, 0 pre-existing"


def issue(number: int = 7, tier: str = fix_issue.MERGE_TIERS[0], assoc: str = "OWNER",
          author: int = OWNER, editor: int | None = None) -> dict:
    return {"number": number, "body": f"### Problem\n\nx\n\n### How far the agent may go\n\n{tier}\n",
            "authorAssociation": assoc, "author": {"databaseId": author},
            "editor": None if editor is None else {"databaseId": editor}}


def labeled(name: str, actor: int, on: bool = True) -> dict:
    return {"__typename": "LabeledEvent" if on else "UnlabeledEvent", "label": {"name": name}, "actor": {"databaseId": actor}}


def repo(body: str = VERDICT, author: int = AGENT, labels=(), events=(), issues=()) -> dict:
    return {"owner": {"databaseId": OWNER}, "holdLabel": {"createdAt": "2026-01-01T00:00:00Z"}, "pullRequest": {
        "body": f"## Review\n\n{body}\n", "headRefOid": HEAD, "createdAt": "2026-09-25T00:00:00Z",
        "author": {"databaseId": author},
        "labels": {"nodes": [{"name": n} for n in labels]},
        "timelineItems": {"filteredCount": len(events), "nodes": list(events)},
        "closingIssuesReferences": {"nodes": list(issues)}}}


class ContractTest(unittest.TestCase):
    def setUp(self):
        self._env = os.environ.get("LOCAL_AGENT_USER_ID")
        os.environ["LOCAL_AGENT_USER_ID"] = str(AGENT)

    def tearDown(self):
        if self._env is None:
            os.environ.pop("LOCAL_AGENT_USER_ID", None)
        else:
            os.environ["LOCAL_AGENT_USER_ID"] = self._env

    def check(self, r: dict) -> list[str]:
        return contract.problems(r, str(AGENT))

    def test_a_clean_agent_pr_passes(self):
        self.assertEqual(self.check(repo(issues=[issue()])), [])

    def test_an_agent_pr_with_no_issue_passes(self):
        self.assertEqual(self.check(repo()), [])

    def test_a_pr_by_anyone_else_passes(self):
        self.assertEqual(self.check(repo(body="", author=OWNER, labels=["hold"])), [])

    def test_no_agent_id_fails_closed(self):
        self.assertIn("LOCAL_AGENT_USER_ID", contract.problems(repo(author=OWNER), "")[0])

    def test_a_missing_verdict_fails(self):
        self.assertIn("no `Review-verdict", self.check(repo(body="pr-reviewer 0 important"))[0])

    def test_a_verdict_for_an_older_commit_fails(self):
        self.assertIn("the head is", self.check(repo(body=VERDICT.replace(HEAD[:7], "1234567")))[0])

    def test_an_important_finding_fails(self):
        self.assertIn("important finding", self.check(repo(body=VERDICT + ", comment-reviewer 1 important"))[0])

    def test_no_important_findings_counts_as_a_tally(self):
        self.assertEqual(self.check(repo(body=f"Review-verdict: pass @ {HEAD[:7]} — pr-reviewer no important findings")), [])

    def test_a_capitalised_important_finding_fails(self):
        self.assertIn("important finding", self.check(repo(body=VERDICT + ", comment-reviewer 1 Important"))[0])

    def test_a_verdict_without_pr_reviewer_fails(self):
        self.assertIn("pr-reviewer", self.check(repo(body=f"Review-verdict: pass @ {HEAD[:7]} — comment-reviewer 0 important"))[0])

    def test_the_last_verdict_line_counts(self):
        stale = VERDICT.replace(HEAD[:7], "1234567")
        self.assertEqual(self.check(repo(body=f"{stale}\n{VERDICT}")), [])

    def test_the_hold_label_fails(self):
        self.assertIn("`hold`", self.check(repo(labels=["hold"]))[0])

    def test_a_hold_removed_by_someone_else_fails(self):
        events = [labeled("hold", OWNER), labeled("hold", AGENT, on=False)]
        self.assertIn("removed by someone else", self.check(repo(events=events))[0])

    def test_a_hold_the_owner_removed_passes(self):
        self.assertEqual(self.check(repo(events=[labeled("hold", OWNER), labeled("hold", OWNER, on=False)])), [])

    def test_a_deleted_hold_label_fails(self):
        r = repo()
        r["holdLabel"] = None
        self.assertIn("no `hold` label", self.check(r)[0])

    def test_a_label_recreated_after_the_pr_opened_fails(self):
        r = repo()
        r["holdLabel"] = {"createdAt": "2026-09-26T00:00:00Z"}
        self.assertIn("newer than this PR", self.check(r)[0])

    def test_label_events_out_of_view_fail(self):
        r = repo(events=[labeled("x", AGENT)])
        r["pullRequest"]["timelineItems"]["filteredCount"] = 101
        self.assertIn("out of view", self.check(r)[0])

    def test_a_hold_hidden_behind_other_label_events_fails(self):
        events = [labeled("hold", OWNER), labeled("hold", AGENT, on=False), labeled("x", AGENT), labeled("x", AGENT, on=False)]
        self.assertIn("removed by someone else", self.check(repo(events=events))[0])

    def test_an_issue_at_open_a_pr_fails(self):
        self.assertIn("not a merge tier", self.check(repo(issues=[issue(tier=fix_issue.HELD_TIER[fix_issue.MERGE_TIERS[0]])]))[0])

    def test_an_issue_edited_by_someone_else_fails(self):
        self.assertIn("last edited", self.check(repo(issues=[issue(editor=AGENT)]))[0])

    def test_an_issue_the_owner_edited_passes(self):
        self.assertEqual(self.check(repo(issues=[issue(author=AGENT, assoc="NONE", editor=OWNER)])), [])

    def test_an_issue_from_outside_the_repository_fails(self):
        self.assertIn("outside the repository", self.check(repo(issues=[issue(assoc="NONE", author=OTHER)]))[0])

    def test_every_problem_is_reported(self):
        self.assertEqual(len(self.check(repo(body="", labels=["hold"], issues=[issue(editor=OTHER)]))), 3)


if __name__ == "__main__":
    unittest.main()
