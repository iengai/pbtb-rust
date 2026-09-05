---
name: pbtb-ship
description: Getting a change from a working tree onto main in this repo — branch naming, the verification gate, rebasing a stale or conflicting PR, pushing and opening/updating/merging the PR with the right GitHub account. Use it whenever the user asks to commit, push, 提交, 开 PR, 更新 PR, 合并, "修一下这个 PR", or hands you a PR that is CONFLICTING; and whenever you are about to run git push, gh pr create, or gh pr merge yourself. Getting the account or the rebase strategy wrong here has produced a 403, a mangled commit subject, and a merge with a red clippy gate — this skill prevents all three.
---

# pbtb ship

## Accounts (the #1 source of friction)

- `kk-xaiondata` is the default gh account and has **no** push rights and no
  `workflow` scope. `iengai` has both.
- Before any `git push`, `gh pr create/merge/comment`, `gh workflow run`,
  `gh secret set`: `gh auth switch --user iengai`. **Switch back** afterwards:
  `gh auth switch --user kk-xaiondata`. Keep both in one command so the switch
  cannot be forgotten.
- Never combine an account switch with a parallel background command that also
  switches; the active account is process-global.

## Branch and commits

- Branch names follow AGENTS.md: `<type>/<kebab-case>` (`feat/…`, `fix/…`,
  `refactor/…`, `chore/…`, `infra/…`). Rename an auto-created `claude/…` branch
  before pushing.
- Commit subjects: `type(scope): imperative summary`, body explains *why* and
  the decision, ends with `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`.
- Feed multi-line messages with `git commit -F - <<'EOF' … EOF` — NOT the
  PowerShell `@'…'@` form, which in bash produces a literal `@` subject.
- One logical change per commit; a rollout that touches domain, wiring, and
  infra is three commits (`feat(domain)`, `feat`, `infra`) so each is reviewable.

## The gate is not optional

Run the project `verify` skill (`bash .claude/skills/verify/scripts/gate.sh`)
and quote its `GATE …` lines in the PR. Do not push a red gate "to fix in CI":
this repo has no clippy/test CI on PRs, so a red gate merges red.

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
