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


REVIEWED = "1111111aaaaaaa"
OLD_VERDICT = f"Review-verdict: pass @ {REVIEWED[:7]} — pr-reviewer 0 important, 1 nit, 0 pre-existing"
COVERED = {"docs/governance.md", "scripts/ops/contract.py"}


def commit(sha: str, subject: str = "review: take the nit", author: int = AGENT, parents=None,
           files=("docs/governance.md",)) -> dict:
    return {"sha": sha, "subject": subject, "author_id": author, "parents": parents, "files": list(files)}


class FakeHistory:
    """GitHubHistory's three reads over a fixed list of commits."""

    def __init__(self, commits: list[dict], covered: set[str]):
        self._commits, self._covered = commits, covered

    def commits(self) -> list[dict]:
        return [{k: v for k, v in c.items() if k != "files"} for c in self._commits]

    def files(self, sha: str) -> list[str]:
        return next(c["files"] for c in self._commits if c["sha"] == sha)

    def covered(self, sha: str) -> set[str]:
        assert sha == REVIEWED, sha
        return set(self._covered)


def history(*later: dict, covered=COVERED, order=None) -> FakeHistory:
    """The PR's commits: one before the reviewed one, the reviewed one, then `later`, each the parent of the next.

    `order`, when given, is the list order GitHub returns (indices into that chain).
    """
    chain = [commit("0000000ffff", "feat: x", parents=["base000"]), commit(REVIEWED, "feat: y")] + list(later)
    for prev, c in zip(chain, chain[1:]):
        if c["parents"] is None:
            c["parents"] = [prev["sha"]]
    return FakeHistory([chain[i] for i in order] if order else chain, covered)


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

    def review_fix(self, *later: dict, body: str = OLD_VERDICT, **kw) -> list[str]:
        return contract.problems(repo(body=body), str(AGENT), history(*later, **kw))

    def test_a_review_fix_after_the_verdict_passes(self):
        self.assertEqual(self.review_fix(commit(HEAD)), [])

    def test_several_review_fixes_pass(self):
        self.assertEqual(self.review_fix(commit("2222222bbb", files=["scripts/ops/contract.py"]), commit(HEAD)), [])

    def test_a_commit_without_the_review_prefix_fails(self):
        self.assertIn("not a `review:` commit", self.review_fix(commit(HEAD, subject="fix: take the nit"))[0])

    def test_a_review_fix_by_someone_else_fails(self):
        self.assertIn("not authored by the local agent App", self.review_fix(commit(HEAD, author=OWNER))[0])

    def test_a_review_fix_outside_the_reviewed_diff_fails(self):
        found = self.review_fix(commit(HEAD, files=["docs/governance.md", ".github/workflows/contract.yml"]))
        self.assertIn("did not cover (.github/workflows/contract.yml)", found[0])

    def test_a_merge_commit_after_the_verdict_fails(self):
        self.assertIn("merge commit", self.review_fix(commit(HEAD, parents=[REVIEWED, "f00f00f"]))[0])

    def test_a_back_dated_commit_listed_before_the_reviewed_one_is_still_checked(self):
        # GitHub lists a cherry-picked head before the commit it sits on top of.
        found = self.review_fix(commit(HEAD, subject="feat: unreviewed", files=["src/main.rs"]), order=[0, 2, 1])
        self.assertIn("not a `review:` commit", found[0])

    def test_a_reviewed_commit_off_the_head_line_fails(self):
        found = self.review_fix(commit(HEAD, parents=["0000000ffff"]))
        self.assertIn("not on the head's first-parent line", found[0])

    def test_one_bad_commit_among_review_fixes_fails(self):
        self.assertIn("not a `review:` commit",
                      self.review_fix(commit("2222222bbb"), commit("3333333ccc", subject="feat: more"), commit(HEAD))[0])

    def test_a_verdict_for_a_sha_outside_the_pr_fails(self):
        body = "Review-verdict: pass @ 9999999 — pr-reviewer 0 important"
        self.assertIn("not one commit of this PR", self.review_fix(commit(HEAD), body=body)[0])

    def test_an_important_finding_still_fails_on_an_older_verdict(self):
        body = f"Review-verdict: pass @ {REVIEWED[:7]} — pr-reviewer 1 important"
        self.assertIn("counts an important finding", self.review_fix(commit(HEAD), body=body)[0])

    def test_unreadable_history_fails(self):
        class Broken:
            def commits(self):
                raise RuntimeError("the PR has 250 commits")
        found = contract.problems(repo(body=OLD_VERDICT), str(AGENT), Broken())
        self.assertIn("could not be read: the PR has 250 commits", found[0])

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


def rest_commit(sha: str, parents=("p0",), author: int | None = AGENT, message: str = "review: x\n\nbody") -> dict:
    return {"sha": sha, "commit": {"message": message}, "author": None if author is None else {"id": author},
            "parents": [{"sha": p} for p in parents]}


class GitHubHistoryTest(unittest.TestCase):
    def history(self, responses: dict) -> contract.GitHubHistory:
        return contract.GitHubHistory("o/r", 5, "base", api=lambda path, *flags: responses[path])

    def test_pages_flatten_into_one_list(self):
        h = self.history({"repos/o/r/pulls/5/commits?per_page=100": [[rest_commit("a1")], [rest_commit("b2", ("a1",))]]})
        self.assertEqual(h.commits(), [
            {"sha": "a1", "subject": "review: x", "author_id": AGENT, "parents": ["p0"]},
            {"sha": "b2", "subject": "review: x", "author_id": AGENT, "parents": ["a1"]}])

    def test_an_unmapped_author_never_matches_the_app(self):
        h = self.history({"repos/o/r/pulls/5/commits?per_page=100": [[rest_commit("a1", author=None)]]})
        self.assertIsNone(h.commits()[0]["author_id"])

    def test_a_commit_list_at_the_cap_fails_closed(self):
        page = [rest_commit(f"{i:040x}") for i in range(contract.LIST_CAP)]
        with self.assertRaisesRegex(RuntimeError, "more than GitHub lists"):
            self.history({"repos/o/r/pulls/5/commits?per_page=100": [page]}).commits()

    def test_a_rename_counts_its_old_path(self):
        h = self.history({"repos/o/r/commits/a1": {"files": [
            {"filename": "docs/new.md", "previous_filename": ".github/workflows/old.yml"}]}})
        self.assertEqual(h.files("a1"), ["docs/new.md", ".github/workflows/old.yml"])

    def test_a_file_list_at_the_cap_fails_closed(self):
        files = [{"filename": f"f{i}"} for i in range(contract.FILES_CAP)]
        with self.assertRaisesRegex(RuntimeError, "more than GitHub lists"):
            self.history({"repos/o/r/commits/a1": {"files": files}}).files("a1")
        with self.assertRaisesRegex(RuntimeError, "more than GitHub lists"):
            self.history({"repos/o/r/compare/base...a1": {"files": files}}).covered("a1")

    def test_the_reviewed_diff_is_base_three_dot_sha(self):
        h = self.history({"repos/o/r/compare/base...a1": {"files": [{"filename": "x"}, {"filename": "y"}]}})
        self.assertEqual(h.covered("a1"), {"x", "y"})


if __name__ == "__main__":
    unittest.main()
