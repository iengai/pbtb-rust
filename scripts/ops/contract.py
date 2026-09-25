"""The `contract` check: whether a local agent PR may merge itself.

  contract.py check --pr N       (CI; env GH_TOKEN, GH_REPO, LOCAL_AGENT_USER_ID)

A PR the local agent App authored (matched by its numeric user id,
LOCAL_AGENT_USER_ID; unset fails every PR) passes only when all of these hold;
every other PR passes, since a person merges it (docs/governance.md § Autonomy
tiers):

  - its body carries `Review-verdict: pass @ <sha>` for the PR's head commit,
    naming pr-reviewer, with every tally on the line at 0 important;
  - the owner has not held it: no `hold` label on it, and the owner's last
    `hold` event on it is not an add. The label must exist and be older than
    the PR, and every label event must fit in one read: deleting and
    recreating the label strips it without an event, and a flood of events
    would push the owner's out of view;
  - every issue it closes asks for a merge tier, was opened by a member or the
    App, and was last edited by its author or the owner, so the tier read is
    the one its author chose.

The verdict line is written by the session that wrote the code: it records
that the review ran on this commit, not that someone else approved it.
Paths agents load and infra need the owner's approval, and that is
`.github/CODEOWNERS` under the ruleset's code-owner review, not this script:
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
      body headRefOid createdAt
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


def verdict_problem(body: str, head: str) -> str:
    """Why the body's review verdict does not clear the head commit, or ""."""
    found = VERDICT.findall(body or "")
    if not found:
        return "no `Review-verdict: pass @ <sha>` line: run the review passes (pbtb-ship) and add it"
    sha, rest = found[-1]
    if not head.startswith(sha):
        return f"the review verdict is for {sha}, the head is {head[:7]}: review the new commits and update the line"
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


def problems(repo: dict, agent_id: str) -> list[str]:
    """Everything that stops this PR merging itself; empty when it may."""
    pr, owner = repo["pullRequest"], actor_id(repo["owner"])
    if not agent_id:
        return ["LOCAL_AGENT_USER_ID is unset, so no PR can be told apart from the agent's"]
    if str(actor_id(pr["author"])) != agent_id:
        return []
    found = [verdict_problem(pr["body"], pr["headRefOid"]), hold_problem(pr, owner, repo["holdLabel"])]
    found += [issue_problem(i, owner) for i in pr["closingIssuesReferences"]["nodes"]]
    return [p for p in found if p]


def fetch(repo_name: str, pr: int) -> dict:
    owner, name = repo_name.split("/", 1)
    out = subprocess.run(
        ["gh", "api", "graphql", "-f", f"query={QUERY}", "-F", f"owner={owner}", "-F", f"name={name}", "-F", f"pr={pr}"],
        check=True, capture_output=True, text=True, encoding="utf-8",
    ).stdout
    return json.loads(out)["data"]["repository"]


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    check = sub.add_parser("check", help="exit 1 when an agent PR may not merge itself")
    check.add_argument("--pr", type=int, required=True)
    a = ap.parse_args()
    found = problems(fetch(os.environ["GH_REPO"], a.pr), os.environ.get("LOCAL_AGENT_USER_ID", "").strip())
    for p in found:
        print(f"::error::{p}")
    print("contract: " + ("; ".join(found) if found else "clear"))
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(main())
