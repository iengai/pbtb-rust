---
name: pbtb-ship
description: Get a change onto main: branch naming, the verify gate, the review passes, rebasing a stale PR, push / PR / merge as the local agent App. Use on commit, push, review, open or update a PR, merge, a CONFLICTING PR, and before running git push, gh pr create or gh pr merge yourself.
---

# pbtb ship

## Identity

- A session acts on GitHub as the local agent App, `pbtb-local-agent[bot]`
  (docs/agents.md § Local agents): plain `git commit`, `git push`,
  `gh pr create/merge/comment` and `gh workflow run` are the bot's, with no
  account to pick. When one fails to authenticate, or the hook refuses it for
  a missing identity, run `python scripts/ops/agent_identity.py status`; it
  names the check that does not hold.
- The person's accounts are not the session's: `gh auth switch/login/token`
  and `git credential` are refused. What the App is not granted (`gh secret
  set`, `gh variable set`, rulesets, repository settings) goes to the owner as
  the exact command to run in their own terminal.

## Branch and commits

- Branch names follow AGENTS.md: `<type>/<kebab-case>` (`feat/…`, `fix/…`,
  `refactor/…`, `chore/…`, `infra/…`). Rename an auto-created `claude/…` branch
  before pushing, and before the PR exists: renaming the branch behind an open
  PR closes that PR.
- Commit subjects: `<type>: <summary>` (lowercase imperative, ≤72 chars, the
  types in docs/conventions.md); the body explains *why* and the decision and
  ends with the `Co-Authored-By:` line the session instructions give you.
- Feed multi-line messages with `git commit -F - <<'EOF' … EOF` — NOT the
  PowerShell `@'…'@` form, which in bash produces a literal `@` subject.
- One logical change per commit; a rollout that touches domain, wiring, and
  infra is three commits (`feat(domain)`, `feat`, `infra`) so each is reviewable.
- A change under `.github/workflows/` goes in its own commit at the branch tip:
  the App cannot push it (docs/agents.md § Local agents). Push the commits
  below it, then hand the owner the command that pushes the tip.

## Design review, for a change to what agents load

An Intent that ticks an invariant box, or whose change adds or edits a file
agents load (`AGENTS.md`, a skill, an agent definition, a hook, `REVIEW.md`,
the CI scripts), gets its design reviewed before the branch exists. Post the
design as a comment on the Intent, then dispatch a `general-purpose` subagent
on it with six questions: conflicts with the repo's rules (one home per fact,
budgets, comments as-is); does each mechanism close its loop, and what happens
when nobody does the human step; failure modes; a simpler alternative; fit with
the flow above; is *Done when* runnable. Its tally and what changed go in the
same thread; the owner's go is the next comment; the PR's **Why** links the
thread.

## The gate is not optional

Run the project `verify` skill (`bash .claude/skills/verify/scripts/gate.sh`)
and quote its `GATE …` lines in the PR. `verify.yml` runs the same script on
every PR as the `gate` check the ruleset requires. A change to a workflow,
`rust-toolchain.toml`, a Dockerfile or `tests/common/` can pass locally and
fail there: the DynamoDB fixture has a testcontainers branch that only CI
exercises, and PR #49 was green locally while 13 tests failed there. Never
push a red gate "to fix in CI".

## Review before the PR

The gate proves the tree builds and the tests pass; the passes in `REVIEW.md`
look for what the gate cannot decide. Run them once the gate is green and the
commits are in their final shape, and again after a push that changes more
than the review asked for.

1. Dispatch the `pr-reviewer` agent (bugs, security, compliance; it reads
   `REVIEW.md` itself). Beside it, `comment-reviewer` when the diff adds or
   changes comments and `architecture-reviewer` when it touches `src/`. All
   three are read-only and independent: run them in parallel.
2. Fix every 🔴 Important finding before the PR exists, or say in the PR why
   it is not one. 🟡 Nits are yours to take or leave. A 🟣 Pre-existing bug
   becomes an Intent issue (`gh issue create --label intent,bug,source:agent
   --body-file`, the template's fields), not part of this PR. When it is
   self-evident (docs/governance.md § Autonomy tiers: confirmed at file:line,
   self-contained, bounded, too large to inline) add `agent:fix` to the
   labels and set the tier field to *Merge when the verify gate and CI are
   green*; the fix workflow takes it from there.
3. The PR body's **Review** section carries each reviewer's tally line and
   what was done with the findings; a finding you overruled is named there, so
   the PR stays the audit record. When no Important is open, it also carries
   the verdict `pr-reviewer` printed for the head commit, with the other
   reviewers' tallies appended: `Review-verdict: pass @ <sha7> — pr-reviewer
   0 important, 2 nit, 0 pre-existing, comment-reviewer 0 important`. A push
   after the review changes the head, and the `contract` check fails until the
   passes run again on it and the line is updated.
4. A finding the reviewer marked REPEAT carries its correction into the layer
   it names, in this PR; `REVIEW.md` owns that rule. Any other finding is
   fixed in the file it cites (docs/conventions.md § Knowledge placement).
5. The PR's **Knowledge** section ends with one `Lesson` line: what would have
   made this PR shorter or safer, with the slug it belongs to, or `none`. It
   is a candidate, not a doc edit; a slug seen again is what promotes it.

## Merging your own PR

A PR the App authored merges itself: once the verdict line is in, arm it with
`gh pr merge <n> --auto --rebase --delete-branch` and move on. GitHub merges
it when `gate`, `contract` and, for a path in `.github/CODEOWNERS`, the
owner's review hold (docs/governance.md § What GitHub enforces). A red
`contract` says why in its log. Do not arm, and say why in the PR body, when
the owner asked in the session to hold it, when it closes an issue at *Open a
PR*, when an Important is still open, or when the diff grew past the task.
Arm only PRs the App authored; the owner's are theirs to merge.

## Rebasing a stale PR

1. `git fetch origin main -q && git rebase origin/main`.
2. Structural commits usually replay clean; **lint-sweep commits collide with
   everything**. For a file where the conflict is mechanical (`format!` arg
   inlining, renames), take main's version (`git checkout --ours -- <file>` during
   a rebase) and re-apply the lint pass with `cargo clippy --fix --allow-dirty
   --allow-staged`, then hand-resolve only what `--fix` cannot (dead code to
   delete, `#[allow(...)]` the PR's own convention already uses).
3. Test-module append/append conflicts: keep both blocks and restore the one
   closing brace the markers swallowed.
4. Squash follow-up fixes into the commit they belong to (`git commit --fixup`
   is unavailable non-interactively; use stash → `reset --hard HEAD~1` → pop →
   `--amend` → `cherry-pick` the rest).
5. Re-run the gate on the rebased tree; then `git push --force-with-lease`.

## PR

- `gh pr create --base main --head <branch> --title "<type>: …" --body "$(cat <<'EOF' … EOF)"`
  with the sections of `.github/pull_request_template.md` (`closes #<n>` in Why
  when the change started from an issue, so the merge closes it; rollout only if
  it changes deployed shape — say the order and the window, see pbtb-deploy).
- GitHub recomputes mergeability asynchronously after a force-push; `CONFLICTING`
  right after pushing is stale — poll `gh pr view --json mergeable` until it
  settles rather than trusting the first answer.
- Merge with `--rebase` to keep the linear history this repo has; squash only
  for a single-commit PR.
- When the change is deployable, the deploy waits for the merge: `gh pr checks
  <n> --watch`, then `gh pr view <n> --json state` reads `MERGED` before you
  hand off to the `pbtb-deploy` skill in the same session. Afterwards `git
  checkout main && git reset --hard origin/main` and delete the local branch.
