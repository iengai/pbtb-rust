# The CI agents

Two workflows run a model on this repository without a person at the keyboard: `incident-diagnose` (read-only, answers an Incident issue) and `issue-fix` (writes a fix, opens a PR). Both drive Claude Code headless (`claude -p`) on a checkout, so the model works with the harness this repository is written for: `CLAUDE.md` and `AGENTS.md`, the skills, the agents and the hooks under `.claude/`. The model is whatever the Anthropic-compatible endpoint serves: DeepSeek's `deepseek-flash` by default (`ANTHROPIC_BASE_URL` / `ANTHROPIC_MODEL`, from the repository variables `LLM_ANTHROPIC_BASE_URL` / `LLM_MODEL` and the secret `LLM_API_KEY`). The harness version is pinned in both workflows (`@anthropic-ai/claude-code@2.1.263`); a release does not change what a run can do until someone moves the pin. Where each runs and how to dispatch one is in the triage skill's component map; this leaf is for changing them.

## What each agent may do

The shared module `scripts/ops/claude_harness.py` builds the harness environment, writes the settings file and provides the hook; each script owns its whitelist and rules.

| | `diagnose_issue.py` | `fix_issue.py run` |
|---|---|---|
| Read / Grep / Glob | inside the checkout only | inside the checkout only |
| Bash | `git log\|show\|blame`; `pbtb_ops.py bot-status\|deploy-audit\|lambda-logs` with validated arguments; `diagnose_issue.py sentry ID` | `cargo test\|check\|clippy\|build\|fmt` with plain arguments; `git diff\|status\|log\|show\|blame`; the gate; `python -m py_compile\|unittest\|doctest`; `python -c` |
| Edit / Write | none | any path not on the deny list (workflows, terraform, hooks, the settings file, the gate, the scripts behind the intake / diagnose / fix workflows and their shared harness module, dependency manifests, build scripts, `.devcontainer/`, `.gitignore`, `.git`, `.cargo`, `AGENTS.md`, `REVIEW.md`) |
| Web, subagents, tasks | none | none |
| Credentials in the environment | the model key; the `gh-diagnose` role's (read-only, for the ops probes); the Sentry token | the model key only: the GitHub token leaves before the harness starts |

The hook (`<script> hook`, wired through the settings file the script writes) decides every tool call and logs every Bash command with its exit code and output to `runs.jsonl`; the "Harness trail" step of each workflow prints the commands, and the artifact carries them. What leaves a run (the answer, the trail, the log) has the model key, the AWS secret and the Sentry token blanked. A hook that cannot decide denies. The whitelist is a scope bound; the safety bounds are which credentials are in the environment and the deny list re-applied to the staged paths in `publish`.

Refusals worth knowing when reading a trail: `ls`, `grep`, `find` (Read / Grep / Glob do that); a pipe, `&&`, `;`, a redirection, `$(…)`; a `--profile` or an environment prefix on an ops probe; any command with a line break (the diagnose agent); `git blame --contents`, `--output`, a `..` path segment; a Read outside the checkout (`/proc/self/environ`, the cargo registry).

## Rules the model gets

Each script appends a `RULES` string as the system prompt: the answer shape the script parses, the whitelist in words, and the habits seen twice. That string is the layer of rules the agent has loaded for every run, whatever the checkout it works on. A habit seen once is a `[cat:agent-habit] Lesson:` line in the run's PR body or comment (docs/conventions.md § Issues); seen twice, it goes into `RULES`, and nowhere else.

## Trying a change

- `gh workflow run issue-fix.yml --ref <branch> -f issue_number=<n> -f dry_run=true` runs the model and the gate on the branch and prints the PR as `run` sees it (the tier hold on what agents load is `publish`'s); nothing is pushed or commented. A real fix runs on `main` only.
- `gh workflow run incident-diagnose.yml --ref <branch> -f issue_number=<n> -f dry_run=true` prints the comment in the log. Off `main` there is no AWS side: the `gh-diagnose` role trusts `refs/heads/main` alone, so the OIDC step is skipped and the ops probes answer with the missing credentials.
- The local `claude` CLI is a different login from the one CI uses; the scripts are tried in CI, not on a workstation. The offline tests of the scripts drive the real hook subcommand with a fake harness.

## Publishing

`issue-fix`'s second job runs no model code: it re-derives the tier from the issue, applies the patch on `fix/issue-<n>`, opens the PR with the secret `FIX_PUBLISH_TOKEN` (a member's fine-grained token, so the PR's own checks run and a merge closes the issue) or, without it, the repository token (the run waits for a maintainer's approval and a merge closes nothing). A merge tier is held at "open a PR" when the issue's author is outside the repository, and when the patch touches `.claude/**` or `docs/**`: a change to what agents load is read by a person before it merges.
