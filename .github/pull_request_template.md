## Why

If this started from an issue: `closes #<n>` (the merge closes it; `refs #<n>` when it only touches one).

## What

## Verification

Paste the `GATE …` lines from `bash .claude/skills/verify/scripts/gate.sh`.

## Review

The tally line from each reviewer run (`pr-reviewer`; `comment-reviewer` / `architecture-reviewer` when the diff called for them) and what was done with the findings. Policy: `REVIEW.md`.

## Rollout

Only if the deployed shape changes: the order and the window (see the `pbtb-deploy` skill).

## Knowledge

- [ ] `AGENTS.md`, the `docs/` leaf for this area, and any skill this change makes stale are updated, or nothing described the old behaviour.
