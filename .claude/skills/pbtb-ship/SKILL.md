---
name: pbtb-ship
description: Get a change onto main: branch naming, the verify gate, the review passes, rebasing a stale PR, push / PR / merge with the right GitHub account. Use on commit, push, review, open or update a PR, merge, a CONFLICTING PR, and before running git push, gh pr create or gh pr merge yourself.
---

# pbtb ship

## Accounts (the #1 source of friction)

- Two GitHub accounts are logged in on this machine. The **repo owner
  account** (`gh repo view --json owner -q .owner.login`; here `iengai`) is the
  default and the only one with push rights and the `workflow` scope; the other
  gets a 403 on push and a "workflow scope" error on dispatch.
- Before any `git push`, `gh pr create/merge/comment`, `gh workflow run`,
  `gh secret set`: confirm with `gh auth status` that `iengai` is active. If it
  is not, `gh auth switch --user iengai` and stay there; do not switch back.
- Never combine an account switch with a parallel background command that also
  switches; the active account is process-global.

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
every PR, so a green local gate is enough to merge without waiting for CI —
except when the change touches a workflow, `rust-toolchain.toml`, a Dockerfile,
or `tests/common/`: the DynamoDB fixture has a testcontainers branch that only
CI exercises, and PR #49 was green locally while 13 tests failed there. Never
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
   self-evident (docs/conventions.md § Issues: confirmed at file:line,
   self-contained, bounded, too large to inline) add `agent:fix` to the
   labels and set the tier field to *Merge when the verify gate and CI are
   green*; the fix workflow takes it from there.
3. The PR body's **Review** section carries each reviewer's tally line and
   what was done with the findings; a finding you overruled is named there, so
   the PR stays the audit record.
4. A finding the reviewer marked REPEAT carries its correction into the layer
   it names, in this PR; `REVIEW.md` owns that rule.
5. The PR's **Knowledge** section ends with one `Lesson` line: what would have
   made this PR shorter or safer, with the slug it belongs to, or `none`. It
   is a candidate, not a doc edit; a slug seen again is what promotes it.

## The merge-when-green tier

A task that arrived as a suggested-task chip, or an issue whose *How far the
agent may go* is *Merge when the verify gate and CI are green*, does not
wait for a person at the PR: run the review passes and the gate as above,
open the PR with `closes #<n>`, wait for the `gate` check (`gh pr checks
<n> --watch`), and merge it yourself with `gh pr merge <n> --rebase
--delete-branch`. An 🔴 Important finding you cannot resolve, a red check,
or a diff that grew past the task's four tests (docs/conventions.md §
Issues) drops the task back to *open a PR*: say so in the PR body and stop.

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
  with sections **Why / What / Verification / Review / Rollout** (`closes #<n>`
  in Why when the change started from an issue, so the merge closes it; rollout
  only if it changes deployed shape — say the order and the window, see pbtb-deploy).
- GitHub recomputes mergeability asynchronously after a force-push; `CONFLICTING`
  right after pushing is stale — poll `gh pr view --json mergeable` until it
  settles rather than trusting the first answer.
- Merge with `gh pr merge <n> --rebase --delete-branch` to keep the linear
  history this repo has; squash only for a single-commit PR.
- After merging: `git checkout main && git reset --hard origin/main`, delete the
  local temp branch, and — if the change is deployable — hand off to the
  `pbtb-deploy` skill in the same session.
