#!/usr/bin/env python3
"""Turn unresolved Sentry issues into GitHub Incident issues.

  list                  print the project's unresolved Sentry issues (no writes)
  file [--dry-run]      open one GitHub issue per Sentry issue not filed yet

Sentry's own GitHub integration files issues only on paid plans, so this is
the pull side of the loop: a scheduled workflow runs `file`, and a human (or
an agent starting from `gh issue view`) triages the result like any other
Incident. The GitHub issue body carries the Sentry issue id in an HTML
comment; that marker is the deduplication key, so re-runs and manual edits
never file the same Sentry issue twice.

Environment:
  SENTRY_AUTH_TOKEN   a token with event:read + project:read (required)
  SENTRY_URL          region base, default https://de.sentry.io
  SENTRY_ORG          default pbtb
  SENTRY_PROJECT      default pbtb-rust
  GH_TOKEN            what `gh` authenticates with; in Actions, github.token
  DIAGNOSE_WORKFLOW   workflow file to dispatch per filed issue (default
                      incident-diagnose.yml; empty disables)

Stdlib only: the workflow runs it on a bare runner with no install step.
"""
from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import urllib.error
import urllib.parse
import urllib.request

SENTRY_URL = os.environ.get("SENTRY_URL", "https://de.sentry.io").rstrip("/")
SENTRY_ORG = os.environ.get("SENTRY_ORG", "pbtb")
SENTRY_PROJECT = os.environ.get("SENTRY_PROJECT", "pbtb-rust")

# The `component` tag every binary sets at init, mapped onto the Incident
# template's "Where" options so a filed issue reads like a human one.
COMPONENT_WHERE = {
    "telebot": "telebot (Telegram)",
    "task-state-change-handler": "restart lambda (task_state_change_handler)",
    "daily-pnl-snapshot": "collector lambda (daily-pnl-snapshot)",
    "mcp-http": "MCP / web API",
    "mcp-stdio": "MCP / web API",
}

LABELS = ["incident", "source:agent"]

# A crash loop can raise many issues between two runs; filing all of them
# at once buries the one that matters. The rest are picked up next run.
MAX_NEW_PER_RUN = 10


def marker(sentry_id: str) -> str:
    return f"<!-- sentry:{sentry_id} -->"


# ---------------------------------------------------------------- sentry


def sentry_get(path: str, **params) -> object:
    token = os.environ.get("SENTRY_AUTH_TOKEN")
    if not token:
        sys.exit("SENTRY_AUTH_TOKEN is not set")
    url = f"{SENTRY_URL}/api/0/{path.lstrip('/')}"
    if params:
        url += "?" + urllib.parse.urlencode(params)
    req = urllib.request.Request(url, headers={"Authorization": f"Bearer {token}"})
    try:
        with urllib.request.urlopen(req, timeout=30) as resp:
            return json.load(resp)
    except urllib.error.HTTPError as e:
        body = e.read().decode("utf-8", errors="replace")[:300]
        sys.exit(f"sentry {e.code} on {path}: {body}")


def unresolved_issues() -> list[dict]:
    return sentry_get(
        f"projects/{SENTRY_ORG}/{SENTRY_PROJECT}/issues/",
        query="is:unresolved",
        sort="date",
        limit=50,
    )


def latest_event(issue_id: str) -> dict:
    return sentry_get(f"organizations/{SENTRY_ORG}/issues/{issue_id}/events/latest/")


def event_tags(event: dict) -> dict[str, str]:
    return {t["key"]: t["value"] for t in event.get("tags", []) if "key" in t}


def event_message(event: dict) -> str:
    """The line the binary logged, or the exception's type and value."""
    for entry in event.get("entries", []):
        if entry.get("type") == "message":
            return entry.get("data", {}).get("formatted") or entry.get("data", {}).get("message", "")
        if entry.get("type") == "exception":
            values = entry.get("data", {}).get("values", [])
            if values:
                v = values[-1]
                return f"{v.get('type', '')}: {v.get('value', '')}".strip(": ")
    return event.get("message") or event.get("title") or ""


# ---------------------------------------------------------------- github


def gh(*args: str) -> str:
    return subprocess.run(
        ["gh", *args], check=True, capture_output=True, text=True, encoding="utf-8"
    ).stdout


def filed_sentry_ids() -> set[str]:
    """Sentry ids already carried by an incident issue, open or closed."""
    out = gh(
        "issue", "list", "--label", "incident", "--state", "all",
        "--limit", "1000", "--json", "body",
    )
    ids = set()
    for issue in json.loads(out):
        body = issue.get("body") or ""
        start = 0
        while True:
            i = body.find("<!-- sentry:", start)
            if i < 0:
                break
            j = body.find(" -->", i)
            if j < 0:
                break
            ids.add(body[i + len("<!-- sentry:"):j])
            start = j
    return ids


def ensure_labels() -> None:
    gh("label", "create", "source:agent", "--force",
       "--description", "Filed by an agent or a workflow, not a person; triage like any other",
       "--color", "5319e7")


def issue_title(issue: dict, tags: dict[str, str]) -> str:
    component = tags.get("component", "unknown")
    title = (issue.get("title") or "").strip()
    return f"incident: [{component}] {title}"[:200]


def issue_body(issue: dict, event: dict, tags: dict[str, str]) -> str:
    message = event_message(event).strip()
    where = COMPONENT_WHERE.get(tags.get("component", ""), "not sure")
    ids = ", ".join(f"{k}={tags[k]}" for k in ("ref_id", "bot_id", "user_id") if k in tags) or "none on the event"
    changed = ", ".join(
        f"{k}={tags[k]}" for k in ("release", "environment") if k in tags
    ) or "release/environment tags absent"
    culprit = issue.get("culprit") or ""
    return f"""{marker(issue['id'])}
### Symptom

```
{message or issue.get('title', '')}
```

{issue.get('shortId', '')}: {issue.get('permalink', '')}
{f'in `{culprit}`' if culprit else ''}

### Where

{where}

### Impact

Unassessed. Filed by the incident-intake workflow from Sentry; the triage decides.

### When

First seen {issue.get('firstSeen', '?')}, last seen {issue.get('lastSeen', '?')} (UTC), {issue.get('count', '?')} events.

### Bot id / user id / task id

{ids}

### What changed recently

{changed}

### How far the agent may go

Diagnose only; report here
"""


def dispatch_diagnosis(number: str) -> None:
    """An issue created with the repository token raises no `issues` event,
    so the diagnosis is started by hand; a failure here leaves the issue
    filed and is only reported."""
    workflow = os.environ.get("DIAGNOSE_WORKFLOW", "incident-diagnose.yml")
    if not workflow or not number.isdigit():
        return
    try:
        gh("workflow", "run", workflow, "-f", f"issue_number={number}")
        print(f"dispatched {workflow} for #{number}", file=sys.stderr)
    except subprocess.CalledProcessError as e:
        print(f"could not dispatch {workflow} for #{number}: {e.stderr.strip()[:200]}", file=sys.stderr)


def file_new(dry_run: bool) -> int:
    issues = unresolved_issues()
    known = filed_sentry_ids()
    new = [i for i in issues if i["id"] not in known]
    print(f"{len(issues)} unresolved in Sentry, {len(new)} not filed yet", file=sys.stderr)
    if not new:
        return 0
    if not dry_run:
        ensure_labels()
    for issue in new[:MAX_NEW_PER_RUN]:
        event = latest_event(issue["id"])
        tags = event_tags(event)
        title = issue_title(issue, tags)
        body = issue_body(issue, event, tags)
        if dry_run:
            print(f"--- would file: {title}\n{body}")
            continue
        args = ["issue", "create", "--title", title, "--body", body]
        for label in LABELS:
            args += ["--label", label]
        url = gh(*args).strip()
        print(f"filed {issue.get('shortId')} -> {url}", file=sys.stderr)
        dispatch_diagnosis(url.rsplit("/", 1)[-1])
    if len(new) > MAX_NEW_PER_RUN:
        print(f"{len(new) - MAX_NEW_PER_RUN} more left for the next run", file=sys.stderr)
    return 0


def list_unresolved() -> int:
    for issue in unresolved_issues():
        print(f"{issue.get('shortId'):16} {issue.get('count'):>5}  {issue.get('lastSeen')}  {issue.get('title')}")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    sub.add_parser("list")
    f = sub.add_parser("file")
    f.add_argument("--dry-run", action="store_true", help="print the issues instead of creating them")
    a = ap.parse_args()
    if a.cmd == "list":
        return list_unresolved()
    return file_new(a.dry_run)


if __name__ == "__main__":
    sys.exit(main())
