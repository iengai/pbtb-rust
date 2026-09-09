---
name: pbtb-ship
description: Get a change from the working tree onto main: branch naming, the verify gate, rebasing a stale or conflicting PR, push / PR / merge with the right GitHub account. Use on commit, push, open or update a PR, merge, a CONFLICTING PR, and before running git push, gh pr create or gh pr merge yourself.
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

## The gate is not optional

Run the project `verify` skill (`bash .claude/skills/verify/scripts/gate.sh`)
and quote its `GATE …` lines in the PR. `verify.yml` runs the same script on
every PR, so a green local gate is enough to merge without waiting for CI —
except when the change touches a workflow, `rust-toolchain.toml`, a Dockerfile,
or `tests/common/`: the DynamoDB fixture has a testcontainers branch that only
CI exercises, and PR #49 was green locally while 13 tests failed there. Never
push a red gate "to fix in CI".

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
  with sections **Why / What / Verification / Rollout** (rollout only if it
  changes deployed shape — say the order and the window, see pbtb-deploy).
- GitHub recomputes mergeability asynchronously after a force-push; `CONFLICTING`
  right after pushing is stale — poll `gh pr view --json mergeable` until it
  settles rather than trusting the first answer.
- Merge with `gh pr merge <n> --rebase --delete-branch` to keep the linear
  history this repo has; squash only for a single-commit PR.
- After merging: `git checkout main && git reset --hard origin/main`, delete the
  local temp branch, and — if the change is deployable — hand off to the
  `pbtb-deploy` skill in the same session.
