"""The `contract` check: whether a local agent PR may merge itself.

  contract.py check --pr N       (CI; env GH_TOKEN, GH_REPO, LOCAL_AGENT_USER_ID)

A PR the local agent App authored (matched by its numeric user id,
LOCAL_AGENT_USER_ID; unset fails every PR) passes only when all of these hold;
every other PR passes, since a person merges it (docs/governance.md § Autonomy
tiers):

  - its body carries `Review-verdict: pass @ <sha>` naming pr-reviewer, with
    every tally on the line at 0 important, for the PR's head commit or for
    an earlier commit on the head's first-parent line when every commit
    between is a review fix: authored by the App, its subject starting
    `review:`, a single parent, and touching only files the reviewed diff
    (the PR's base...<sha>) touched. A nit fixed after the review does not
    re-trigger it;
  - the owner has not held it: no `hold` label on it, and the owner's last
    `hold` event on it is not an add. The label must exist and be older than
    the PR, and every label event must fit in one read: deleting and
    recreating the label strips it without an event, and a flood of events
    would push the owner's out of view;
  - every issue it closes asks for a merge tier, was opened by a member or the
    App, and was last edited by its author or the owner, so the tier read is
    the one its author chose.

The verdict line is written by the session that wrote the code: it records
that the review ran on that commit (and that only review fixes followed),
not that someone else approved it. The paths in `.github/CODEOWNERS` need
the owner's approval, and that is the ruleset's code-owner review, not this
script:
a PR can rewrite this file or its workflow, and its own run would use them.
"""
from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import fix_issue  # noqa: E402

HOLD = "hold"
REVIEWER = "pr-reviewer"
REVIEW_FIX = "review:"
# GitHub lists at most this many commits of a PR and files of a commit or a
# comparison; a longer list could hide what a review fix touched.
LIST_CAP = 250
FILES_CAP = 300
VERDICT = re.compile(r"^Review-verdict:\s*pass\s*@\s*([0-9a-f]{7,40})\b(.*)$", re.M)
TALLY = re.compile(r"(\d+) important\b|no important findings", re.I)
EVENTS = 100

ACTOR = "... on User { databaseId } ... on Bot { databaseId }"
QUERY = f"""
query($owner: String!, $name: String!, $pr: Int!) {{
  repository(owner: $owner, name: $name) {{
    owner {{ ... on User {{ databaseId }} ... on Organization {{ databaseId }} }}
    holdLabel: label(name: "{HOLD}") {{ createdAt }}
    pullRequest(number: $pr) {{
      body headRefOid baseRefOid createdAt
      author {{ {ACTOR} }}
      labels(first: 50) {{ nodes {{ name }} }}
      timelineItems(itemTypes: [LABELED_EVENT, UNLABELED_EVENT], last: {EVENTS}) {{
        filteredCount
        nodes {{
          __typename
          ... on LabeledEvent {{ label {{ name }} actor {{ {ACTOR} }} }}
          ... on UnlabeledEvent {{ label {{ name }} actor {{ {ACTOR} }} }}
        }}
      }}
      closingIssuesReferences(first: 20) {{
        nodes {{ number body authorAssociation author {{ {ACTOR} }} editor {{ {ACTOR} }} }}
      }}
    }}
  }}
}}
"""


def actor_id(node: dict | None) -> object:
    return (node or {}).get("databaseId")


def review_fix_problem(sha: str, head: str, history, agent_id: str) -> str:
    """Why the commits between the reviewed one and the head are not all review fixes, or "".

    The commits checked are the head's first-parent line back to the reviewed
    commit, never the PR's list order, which a back-dated commit can reorder.
    `history` is a `GitHubHistory` (or a fake of its three methods).
    """
    stale = f"the review verdict is for {sha}, the head is {head[:7]}"
    by_sha = {c["sha"]: c for c in history.commits()}
    hits = [s for s in by_sha if s.startswith(sha)]
    if len(hits) != 1:
        return f"{stale}, and {sha} is not one commit of this PR: review the new commits and update the line"
    reviewed, line, at = hits[0], [], head
    while at != reviewed:
        c = by_sha.get(at)
        if c is None:
            return f"{stale}, and {sha} is not on the head's first-parent line: review the new commits and update the line"
        if len(c["parents"]) != 1:
            return f"{stale}: {at[:7]} is a merge commit"
        line.append(c)
        at = c["parents"][0]
    covered = history.covered(reviewed)
    for c in line:
        short = c["sha"][:7]
        if not c["subject"].startswith(REVIEW_FIX):
            return f"{stale}: {short} is not a `{REVIEW_FIX}` commit, so review the new commits and update the line"
        if str(c["author_id"]) != agent_id:
            return f"{stale}: {short} was not authored by the local agent App"
        outside = sorted(set(history.files(c["sha"])) - covered)
        if outside:
            return (f"{stale}: {short} touches files the review did not cover "
                    f"({', '.join(outside[:3])}{', …' if len(outside) > 3 else ''}), so review it and update the line")
    return ""


def verdict_problem(body: str, head: str, history=None, agent_id: str = "") -> str:
    """Why the body's review verdict does not clear the head commit, or "".

    With `history` a verdict for an earlier commit clears the head when
    `review_fix_problem` finds nothing; without it the verdict must name the head.
    """
    found = VERDICT.findall(body or "")
    if not found:
        return "no `Review-verdict: pass @ <sha>` line: run the review passes (pbtb-ship) and add it"
    sha, rest = found[-1]
    if not head.startswith(sha):
        if history is None:
            return f"the review verdict is for {sha}, the head is {head[:7]}: review the new commits and update the line"
        try:
            problem = review_fix_problem(sha, head, history, agent_id)
        except (RuntimeError, subprocess.CalledProcessError) as e:
            return f"the review verdict is for {sha}, the head is {head[:7]}, and the commits between could not be read: {e}"
        if problem:
            return problem
    if REVIEWER not in rest:
        return f"the review verdict does not name {REVIEWER}"
    tallies = TALLY.findall(rest)
    if not tallies:
        return "the review verdict carries no tally"
    if any(n and int(n) for n in tallies):
        return "the review verdict counts an important finding"
    return ""


def hold_problem(pr: dict, owner: object, label: dict | None) -> str:
    if label is None:
        return f"the repository has no `{HOLD}` label, so no hold can be seen: recreate it"
    if label["createdAt"] > pr["createdAt"]:
        return f"the `{HOLD}` label is newer than this PR, so an earlier hold may be gone: the owner merges it"
    timeline = pr["timelineItems"]
    if timeline["filteredCount"] > len(timeline["nodes"]):
        return f"more than {EVENTS} label events, so the owner's could be out of view: the owner merges it"
    if any(n["name"] == HOLD for n in pr["labels"]["nodes"]):
        return f"the `{HOLD}` label is on: the owner merges it"
    owner_events = [e for e in timeline["nodes"]
                    if (e.get("label") or {}).get("name") == HOLD and actor_id(e["actor"]) == owner]
    if owner_events and owner_events[-1]["__typename"] == "LabeledEvent":
        return f"the owner's `{HOLD}` label was removed by someone else"
    return ""


def issue_problem(issue: dict, owner: object) -> str:
    n = issue["number"]
    tier = fix_issue.tier_field(issue["body"] or "")
    if not fix_issue.tier_allows_merge(tier):
        return f"#{n} asks for *{tier or 'no tier'}*, not a merge tier"
    author = actor_id(issue["author"])
    if not fix_issue.trusted(issue["authorAssociation"], author):
        return f"#{n} was opened by someone outside the repository ({issue['authorAssociation']})"
    editor = actor_id(issue["editor"])
    if editor is not None and editor not in (author, owner):
        return f"#{n} was last edited by someone other than its author or the owner, so its tier is not the author's"
    return ""


def problems(repo: dict, agent_id: str, history=None) -> list[str]:
    """Everything that stops this PR merging itself; empty when it may."""
    pr, owner = repo["pullRequest"], actor_id(repo["owner"])
    if not agent_id:
        return ["LOCAL_AGENT_USER_ID is unset, so no PR can be told apart from the agent's"]
    if str(actor_id(pr["author"])) != agent_id:
        return []
    found = [verdict_problem(pr["body"], pr["headRefOid"], history, agent_id),
             hold_problem(pr, owner, repo["holdLabel"])]
    found += [issue_problem(i, owner) for i in pr["closingIssuesReferences"]["nodes"]]
    return [p for p in found if p]


def gh_json(*args: str):
    out = subprocess.run(["gh", "api", *args], check=True, capture_output=True, text=True, encoding="utf-8").stdout
    return json.loads(out)


def fetch(repo_name: str, pr: int) -> dict:
    owner, name = repo_name.split("/", 1)
    return gh_json("graphql", "-f", f"query={QUERY}", "-F", f"owner={owner}", "-F", f"name={name}",
                   "-F", f"pr={pr}")["data"]["repository"]


class GitHubHistory:
    """A PR's commits and files, read through the REST API: the workflow's checkout is sparse and shallow."""

    def __init__(self, repo_name: str, pr: int, base: str, api=None):
        self.repo, self.pr, self.base, self.api = repo_name, pr, base, api or gh_json

    def commits(self) -> list[dict]:
        """{sha, subject, author_id, parents: [sha]} for each commit of the PR."""
        pages = self.api(f"repos/{self.repo}/pulls/{self.pr}/commits?per_page=100", "--paginate", "--slurp")
        raw = [c for page in pages for c in page]
        if len(raw) >= LIST_CAP:
            raise RuntimeError(f"the PR has {len(raw)} commits, more than GitHub lists in full")
        return [{"sha": c["sha"], "subject": c["commit"]["message"].split("\n", 1)[0],
                 "author_id": (c.get("author") or {}).get("id"), "parents": [p["sha"] for p in c["parents"]]}
                for c in raw]

    def files(self, sha: str) -> list[str]:
        """The paths a commit touches; a rename counts its old path too."""
        files = self.api(f"repos/{self.repo}/commits/{sha}")["files"]
        if len(files) >= FILES_CAP:
            raise RuntimeError(f"{sha[:7]} touches {len(files)} files, more than GitHub lists in full")
        return [f["filename"] for f in files] + [f["previous_filename"] for f in files if f.get("previous_filename")]

    def covered(self, sha: str) -> set[str]:
        """The files of the reviewed diff: the PR's base...sha, from their merge base."""
        files = self.api(f"repos/{self.repo}/compare/{self.base}...{sha}")["files"]
        if len(files) >= FILES_CAP:
            raise RuntimeError(f"the reviewed diff touches {len(files)} files, more than GitHub lists in full")
        return {f["filename"] for f in files}


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    check = sub.add_parser("check", help="exit 1 when an agent PR may not merge itself")
    check.add_argument("--pr", type=int, required=True)
    a = ap.parse_args()
    repo_name = os.environ["GH_REPO"]
    repo = fetch(repo_name, a.pr)
    history = GitHubHistory(repo_name, a.pr, repo["pullRequest"]["baseRefOid"])
    found = problems(repo, os.environ.get("LOCAL_AGENT_USER_ID", "").strip(), history)
    for p in found:
        print(f"::error::{p}")
    print("contract: " + ("; ".join(found) if found else "clear"))
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(main())
