# Agent governance

How work reaches `main` when agents do most of it: who may act, the path a change takes, how far an agent may go without a person, and what GitHub enforces so that stays true. The procedures that apply these rules are the `pbtb-ship` skill (branch, gate, review, PR, merge), `REVIEW.md` (the review policy) and [agents.md](agents.md) (the CI agents' bounds and the local agent identity); where a fact learned along the way is written down is [conventions.md](conventions.md) § Knowledge placement.

## Who acts

| Actor | Acts as | May | Bounded by |
|-------|---------|-----|------------|
| The owner | their own GitHub account | set an issue's intent and tier, hold or approve a PR, merge, deploy, trade; secrets, variables, rulesets, repository settings | — |
| A local session (Claude Code on the workstation) | the App `pbtb-local-agent[bot]` | branch, commit, push, open PRs and arm them to merge themselves, file and label issues, dispatch workflows | the checks the ruleset requires (§ What GitHub enforces); the `guard-shell` hook; the App's grant ([agents.md](agents.md) § Local agents) |
| `incident-diagnose` (CI) | read-only probes | one diagnosis comment on an Incident | its whitelist and credentials ([agents.md](agents.md) § What each agent may do) |
| `issue-fix` (CI) | the model writes a patch; a model-free job publishes it | open `fix/issue-<n>`, arm auto-merge at a merge tier | its whitelist, deny list and the publish holds ([agents.md](agents.md) § Publishing) |
| Reviewer agents (`pr-reviewer`, `comment-reviewer`, `architecture-reviewer`) | read-only subagents | report findings in `REVIEW.md`'s shape; `pr-reviewer` prints the verdict line | `REVIEW.md` |

Trading actions (starting, stopping, restarting a live bot, a `RunTask` / `StopTask` by hand) are never delegated through an issue or a PR; they stay with the owner.

## The path of a change

1. **Intake.** An issue from one of the templates (§ Issues): the owner's intent, an agent's own finding (`source:agent`), or a Sentry error the intake files.
2. **Design review**, when the change ticks an invariant box or edits a file agents load: posted on the Intent before the branch exists (`pbtb-ship`, design review). The owner's go is the next comment on the thread, or their words in the session.
3. **Branch and commits** in the format of [conventions.md](conventions.md) § Git Workflow.
4. **The verify gate** (`bash .claude/skills/verify/scripts/gate.sh`), green before any push.
5. **Review passes** from `REVIEW.md` through the reviewer agents, on the final commits. An 🔴 Important is fixed or argued in the PR; the tallies go in the PR's Review section, with the `Review-verdict` line when no Important is open.
6. **The PR** from `.github/pull_request_template.md`: Why (`closes #<n>`), What, Verification (the `GATE …` lines), Review, Rollout, Knowledge (the `Lesson` line).
7. **Merge.** A local agent PR is armed to merge itself (`gh pr merge <n> --auto --rebase --delete-branch`); GitHub merges it once every required check and review holds. Anyone else's PR is merged by a person.
8. **Deploy**, when the change is deployable, in the `pbtb-deploy` order, by whoever the tier names.
9. **Close**: the `Lesson` line in the PR; an Incident also leaves a command in the triage skill's symptom playbooks.

## Autonomy tiers

An issue's *How far the agent may go* field grants a tier; a label only triggers the work.

| Tier | Starts from | The human gate |
|------|-------------|----------------|
| Diagnose only | every Incident a collaborator opens or the intake files | the owner reads the diagnosis comment |
| Open a PR | a collaborator's `agent:fix` label or `issue-fix` dispatch; an issue at this tier | the owner reviews and merges |
| Merge when the verify gate and CI are green | the Intent's tier field; a self-evident finding filed by an agent; a session task with no issue | none past § What GitHub enforces |
| Merge and deploy to dev | the Intent's tier field | as above; the deploy follows `pbtb-deploy`, and `issue-fix` never deploys |

- **A local session's PR merges itself by default.** Once the review passes ran on its head commit with no Important open, the session arms auto-merge and does not wait for a person. It does not arm a PR the owner asked in the session to hold, one that closes an issue at *Open a PR*, or one whose review it could not clear; it says why in the PR body and stops. It arms only PRs the App authored.
- **The diagnose agent.** `incident-diagnose` answers every Incident with a read-only diagnosis comment. It is evidence for the owner's triage, not a verdict; a higher tier starts from a person, or from the diagnosis itself when the finding is self-evident.
- **The fix agent.** At *Open a PR* or above, `issue-fix` writes the change on a clean checkout of `main` and opens `fix/issue-<n>`. Only collaborators' comments on the issue reach the model. A change under `src/` or `tests/` becomes a PR only after a `cargo test` was seen red and then green, any change only after a green gate; the PR carries the gate lines, the red→green evidence, a `REVIEW.md` pass by the same model and `closes #<n>`. At a merge tier the PR is armed to merge itself when `gate` passes, unless the patch touches `.claude/**` or `docs/**`, which a person reads first.
- **Self-evident findings.** A finding is self-evident when its body would pass as a task one agent hands another session: confirmed at `file:line`, not a hunch; self-contained, so a session can act on the body alone and run its *Done when*; bounded, one component, none of the template's invariant boxes ticked, inside the paths the fix agent may write; and too large to fold into the PR that found it. It is filed with `agent:fix` already on and the tier *Merge when the verify gate and CI are green*. An incident the diagnosis finds self-evident is raised one step, to *Open a PR*, since a runtime fault under `src/` deserves a person's eyes before merge.
- **Who may raise a tier.** An agent raises the tier only of an issue an agent filed (`source:agent`); a person's choice stands. A merge tier counts only when the issue's author is a member of the repository or the local agent App, matched by its user id ([agents.md](agents.md) § Local agents): anyone can open an issue here and pick a tier.

## What GitHub enforces

The ruleset on `main` is the boundary; a hook or a skill only keeps a session from walking into it.

- **`gate`** (`verify.yml`): the verify gate on the PR's merge commit.
- **`contract`** (`contract.yml`, `scripts/ops/contract.py`): a PR the local agent App authored passes only when its body carries `Review-verdict: pass @ <head sha>` naming `pr-reviewer` with every tally at 0 important; no `hold` label is on it and the owner's last `hold` event on it is not an add, the label exists and is older than the PR, and the PR's label events fit in one read; and every issue it closes asks for a merge tier, has a trusted author, and was last edited by that author or the owner. Every other PR passes; a person merges it. The check runs only the three code-owned files it needs, so no other file under `scripts/ops/` can shadow a module it imports.
- **Repository settings** *Allow auto-merge* and *Automatically delete head branches*: without the first, `gh pr merge --auto` is refused.
- **Code-owner review** (`.github/CODEOWNERS`): a change to what agents load (`.claude/`, `AGENTS.md`, `REVIEW.md`), to the workflows and the scripts that decide a merge, to `terraform/`, `.devcontainer/` or the toolchain needs the owner's approving review. A PR's own run uses its own copy of a workflow and of `contract.py`, so this, not `contract`, is what keeps a PR from rewriting the check that judges it.
- **The owner holds a PR** with the `hold` label, and releases it by removing the label. It is also the stop for a PR armed before its issue's tier changed: `contract` reads the closing issues when a PR event runs it, not when the issue is edited.
- The owner may merge past the ruleset (the admin bypass, for pull requests only); a merge by the owner with a red check is the owner's call, not a fault.

What this does not prove, by design:

- The `Review-verdict` line is written by the session that wrote the code: it records that the review ran on this commit, not that someone other than the author approved it.
- A session can leave `closes #<n>` out of a PR, or file its own issue at a merge tier; `gate`, the code-owner paths and the PR record bound both.
- An allow rule for `Bash(gh pr merge * --auto *)`, set in Claude Code settings by the owner, lets a session arm a merge without asking, which reaches the owner's own PRs too; a PR of the owner's merged by the App (`mergedBy`) breaks this policy.
- The boundary rests on the App's grant ([agents.md](agents.md) § Local agents): `checks: read` and no `workflows: write`. With `workflows: write`, a workflow it pushes to any branch runs before any review and can ask its own token for `checks: write`, then post a green `contract` or `gate` on another PR's head; that grant is never given back.

## Issues

An issue is the interface between human judgment and agent execution: one change's intent, its lifecycle, and the accept-or-reject decision. It is not a place knowledge lives; what is learned while closing it goes to a layer in [conventions.md](conventions.md) § Knowledge placement.

- One template per kind of work, in `.github/ISSUE_TEMPLATE/`: **Intent** (a change: problem, outcome, *done when*, out of scope, invariants touched, how far the agent may go), **Incident** (a symptom, verbatim; the agent starts with `pbtb-triage` and delivers a diagnosis before a patch), **Rollout** (what, window, rollback, done when), **Knowledge drift** (what is written, what is true, which layer gets the fact). Blank issues are off.
- Every template carries a *done when* an agent can run and an autonomy tier.
- An agent starts from `gh issue view <n>`, restates *done when* in its PR, and closes with `closes #<n>`. The restatement lists every bullet with the evidence that meets it (a command and its output, a timing); a bullet whose measurement was not taken is listed as unmet, never implied by a docs sentence. A bullet that names what a test asserts is met only by a test that runs the production implementation; a fake standing in for that implementation does not meet it. A PR that changes a behaviour an issue's *done when* pins edits that line in the same PR, so the *done when* stays runnable against `main`.
- Findings an agent makes on its own (a red not in its diff, a `deploy-audit` finding, a stale doc) go in through the same templates via `gh issue create --body-file`, with the label `source:agent` beside the template's own. Three label axes, one each: the template's label says which flow the issue enters (`intent`, `incident`, `rollout`, `knowledge`), `bug` / `enhancement` say what kind of change it asks for, `source:agent` says who filed it. A defect a review finds is `intent` + `bug` + `source:agent`: it wants a change with a known cause, not a diagnosis.
- Runtime errors arrive the same way: every binary reports `tracing::error!` events to Sentry, and the `incident-intake` workflow (`scripts/ops/sentry_issues.py`, every six hours) files one Incident issue per unresolved Sentry issue, labelled `incident` + `source:agent`, at the "diagnose only" tier. The Sentry id sits in the body as `<!-- sentry:<id> -->`; that marker is what stops a second filing, so leave it in when editing. Resolving the Sentry issue is a human act, done once the GitHub issue closes.
- An Incident closes with a line in the matching section of the triage skill's symptom playbooks saying what to check first next time, as a command; the narrative stays on the issue, and the closing comment links the PR that added the line or says `nothing new`.
- Every PR ends with one `Lesson` line in its Knowledge section (the fix agent's and the diagnose agent's answers carry the same line): what would have made the work shorter or safer, with a slug from `REVIEW.md`, or `none`. It is a candidate: the review that meets the slug again (`REVIEW.md`, repeats) is what moves the correction into the layer [conventions.md](conventions.md) § Knowledge placement names. A model habit of a CI agent seen twice goes into that agent's `RULES` ([agents.md](agents.md) § Rules the model gets).
