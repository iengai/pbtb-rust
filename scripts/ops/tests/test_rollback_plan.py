"""rollback-plan finds the commits a merged PR became and refuses what an agent may not roll back alone.

    python -m unittest scripts/ops/tests/test_rollback_plan.py
"""
from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import pbtb_ops  # noqa: E402

OWNED = pbtb_ops.codeowners("# owners\n/.github/   @o\n/.claude/   @o\n/AGENTS.md  @o\n/terraform/ @o\n")


def commit(sha: str, subject: str, parents: int = 1) -> dict:
    return {"sha": sha, "parents": [f"p{i}" for i in range(parents)], "subject": subject}


def pr(number: int = 7, title: str = "feat: a thing", headlines=("feat: a thing",), files=("docs/x.md",)) -> dict:
    return {"number": number, "title": title, "merge_sha": "c1", "headlines": list(headlines), "files": list(files)}


class RangeTest(unittest.TestCase):
    def test_a_squash_merge_is_its_one_commit(self):
        p = pr(headlines=("a", "b", "c"))
        history = [commit("c1", "feat: a thing (#7)"), commit("c0", "docs: someone else's")]
        self.assertEqual(pbtb_ops.revert_range(p, history, {"docs/x.md"}), (["c1"], ""))

    def test_a_squash_of_another_pr_number_is_not_this_pr(self):
        history = [commit("c1", "feat: a thing (#8)"), commit("c0", "a")]
        self.assertEqual(pbtb_ops.revert_range(pr(headlines=("a", "b")), history, {"docs/x.md"})[0], [])

    def test_a_rebase_merge_is_the_pr_commits(self):
        p = pr(headlines=("one", "two"))
        history = [commit("c1", "two"), commit("c0", "one"), commit("b9", "older")]
        self.assertEqual(pbtb_ops.revert_range(p, history, {"docs/x.md"}), (["c1", "c0"], ""))

    def test_a_cut_headline_matches_its_full_subject(self):
        p = pr(headlines=("fix: document the reference fields and keep a failed reference from r…",))
        history = [commit("c1", "fix: document the reference fields and keep a failed reference from retrying")]
        self.assertEqual(pbtb_ops.revert_range(p, history, {"docs/x.md"})[0], ["c1"])

    def test_other_subjects_in_the_range_refuse(self):
        p = pr(headlines=("one", "two"))
        history = [commit("c1", "two"), commit("c0", "someone else's")]
        self.assertIn("subjects differ", pbtb_ops.revert_range(p, history, {"docs/x.md"})[1])

    def test_other_files_in_the_range_refuse(self):
        p = pr(headlines=("one",))
        self.assertIn("other files", pbtb_ops.revert_range(p, [commit("c1", "one")], {"docs/x.md", "src/y.rs"})[1])

    def test_a_merge_commit_in_the_range_refuses(self):
        p = pr(headlines=("one",))
        self.assertIn("merge commit", pbtb_ops.revert_range(p, [commit("c1", "one", parents=2)], {"docs/x.md"})[1])

    def test_history_not_at_the_merge_commit_refuses(self):
        self.assertIn("does not start", pbtb_ops.revert_range(pr(), [commit("zz", "x")], set())[1])


class RefusalTest(unittest.TestCase):
    def test_docs_and_site_pass(self):
        self.assertEqual(pbtb_ops.rollback_refusals(pr(files=("docs/x.md", "site/src/a.tsx")), OWNED, []), [])

    def test_terraform_refuses(self):
        found = pbtb_ops.rollback_refusals(pr(files=("terraform/envs/dev/nat.tf",)), OWNED, [])
        self.assertTrue(any("never revert and apply" in f for f in found))

    def test_a_workflow_refuses_as_code_owned(self):
        found = pbtb_ops.rollback_refusals(pr(files=(".github/workflows/x.yml",)), OWNED, [])
        self.assertTrue(any("code-owned" in f for f in found))

    def test_a_wildcard_code_owner_entry_refuses(self):
        owned = pbtb_ops.codeowners("/.claude/hooks/ @o\n/.claude/settings*.json @o\n")
        for f in (".claude/settings.json", ".claude/settings.local.json"):
            found = pbtb_ops.rollback_refusals(pr(files=(f, "src/usecase/runbot.rs")), owned, [])
            self.assertTrue(any("code-owned" in x for x in found), f)
        self.assertFalse(pbtb_ops.codeowned(".claude/skills/pbtb-ship/SKILL.md", owned))

    def test_the_stored_shape_refuses(self):
        for f in ("src/domain/bot.rs", "src/infra/botrepository.rs", "src/infra/showcasestore.rs", "docs/data-model.md",
                  "src/bin/daily_pnl_snapshot/model.rs", "src/bin/daily_pnl_snapshot/s3_writer.rs",
                  "src/bin/daily_pnl_snapshot/archive.rs", "src/bin/hl_candle_collector/store.rs"):
            found = pbtb_ops.rollback_refusals(pr(files=(f,)), OWNED, [])
            self.assertTrue(any("stored shape" in x for x in found), f)

    def test_the_env_shape_refuses(self):
        for f in ("src/config/ecs.rs", "src/main.rs", "src/bin/daily_pnl_snapshot/config.rs",
                  "src/bin/shared/mcp_deps.rs", "src/observability.rs"):
            found = pbtb_ops.rollback_refusals(pr(files=(f,)), OWNED, [])
            self.assertTrue(any("env shape" in x for x in found), f)

    def test_a_usecase_change_passes(self):
        self.assertEqual(pbtb_ops.rollback_refusals(pr(files=("src/usecase/runbot.rs",)), OWNED, []), [])

    def test_a_revert_of_a_revert_refuses(self):
        found = pbtb_ops.rollback_refusals(pr(title="revert: feat: a thing (#6)"), OWNED, [])
        self.assertTrue(any("revert of a revert" in f for f in found))

    def test_a_ticked_invariant_refuses(self):
        body = ("### Invariants this change comes near\n\n- [ ] Terraform\n- [X] A row shape in the bots table\n\n"
                "### How far the agent may go\n\nx\n")
        inv = pbtb_ops.ticked_invariants(body)
        self.assertEqual(inv, ["A row shape in the bots table"])
        self.assertTrue(any("invariant" in f for f in pbtb_ops.rollback_refusals(pr(), OWNED, inv)))

    def test_no_ticked_box_is_no_invariant(self):
        self.assertEqual(pbtb_ops.ticked_invariants("### Invariants this change comes near\n\n- [ ] A\n"), [])


class TelebotPinTest(unittest.TestCase):
    def test_no_recorded_sha_refuses(self):
        self.assertIn("--running-sha", pbtb_ops.telebot_pin_problem(None, False, ["c1"], []))

    def test_a_pin_from_after_the_pr_refuses(self):
        self.assertIn("not an ancestor", pbtb_ops.telebot_pin_problem("a" * 40, False, ["c1"], []))

    def test_only_the_pr_commits_since_the_pin_pass(self):
        self.assertEqual(pbtb_ops.telebot_pin_problem("a" * 40, True, ["c1", "c0"], ["c1", "c0"]), "")

    def test_another_image_change_since_the_pin_refuses(self):
        found = pbtb_ops.telebot_pin_problem("a" * 40, True, ["c1"], ["c2", "c1"])
        self.assertIn("1 other commit", found)
        self.assertIn("c2", found)


class PlanTest(unittest.TestCase):
    def plan(self, files, sha=None) -> str:
        return "\n".join(pbtb_ops.rollback_plan(pr(files=files), ["c1"], sha))

    def test_telebot_is_pinned_to_the_recorded_sha_never_latest(self):
        out = self.plan(("src/usecase/runbot.rs",), sha="a" * 40)
        self.assertIn("telebot-deploy.yml --ref main -f tag=" + "a" * 40, out)
        self.assertNotIn("tag=latest", out)

    def test_a_shared_rust_change_names_the_lambdas_for_the_owner(self):
        out = self.plan(("src/usecase/runbot.rs",))
        self.assertIn("task_state_change_handler: the owner's call", out)
        self.assertIn("mcp_http: the owner's call", out)
        self.assertIn("daily_pnl_snapshot, after the revert merges", out)

    def test_a_collector_only_change_redeploys_the_collector_only(self):
        out = self.plan(("src/bin/daily_pnl_snapshot/archive.rs",))
        self.assertIn("daily-pnl-snapshot-deploy.yml", out)
        self.assertNotIn("telebot", out)
        self.assertNotIn("owner's call", out)

    def test_a_dockerfile_only_change_names_every_binary(self):
        out = self.plan((".devcontainer/Dockerfile",), sha="a" * 40)
        self.assertIn("telebot-deploy.yml --ref main -f tag=" + "a" * 40, out)
        self.assertIn("task_state_change_handler: the owner's call", out)
        self.assertNotIn("nothing deployed to redeploy", out)

    def test_site_rides_the_push(self):
        self.assertIn("pages-publish runs on the push", self.plan(("site/src/a.tsx",)))

    def test_docs_redeploy_nothing(self):
        self.assertIn("nothing deployed to redeploy", self.plan(("docs/x.md",)))

    def test_a_quote_in_the_title_stays_one_argument(self):
        out = "\n".join(pbtb_ops.rollback_plan(pr(title="docs: the PR's line"), ["c1"], None))
        self.assertIn("git commit -m 'revert: the PR'\"'\"'s line (#7)'", out)

    def test_the_subject_drops_the_type_and_fits_72(self):
        self.assertEqual(pbtb_ops.revert_subject("feat(domain): add a thing", 9), "revert: add a thing (#9)")
        long = pbtb_ops.revert_subject("docs: " + "x" * 100, 218)
        self.assertEqual(len(long), 72)
        self.assertTrue(long.endswith("\u2026 (#218)"))


if __name__ == "__main__":
    unittest.main()
