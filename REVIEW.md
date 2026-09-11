# Review instructions

Policy for every review of a change to this repo, whether a reviewer agent in `.claude/agents/`, Claude Code Review, or a person runs it. The `pbtb-ship` skill runs these passes before a PR is opened; the PR body's Review section carries the tally.

## Passes, in order

1. **Bugs.** Logic errors, broken edge cases, regressions. Where they hide here: a DynamoDB condition expression or projection that no longer matches the row shape (docs/data-model.md); the start lock claimed after `RunTask` or not at all; desired state (`Bot.enabled`) read as observed state (`BotRuntime`); an error flattened to a string so its retryability is lost; a Telegram dialogue state no callback can reach; a workflow step whose failure is swallowed; an ops-script whitelist regex that admits a shell metacharacter.
2. **Security.** `user_id` taken from client input instead of the authenticated principal; `api_key` / `secret_key` reaching a log line, an error, a `Debug` impl, a DynamoDB projection or an S3 read path; a launch path without the start lock; a public route (web API, Pages data) returning a bot config or its description; a terraform policy wider than the role's stated purpose (the `gh-diagnose` role stays read-only, no SSM); a workflow with more `permissions` than its steps use, or one triggered by an issue or comment without an `author_association` gate.
3. **Compliance.** The diff does what the issue's *Done when* and the PR's **What** say, no more; every AGENTS.md invariant the change comes near is respected; the layering holds (`architecture-reviewer`); new comments survive without the diff (`comment-reviewer`); the docs leaf, skill, or invariant that described the old behaviour is updated (the PR template's Knowledge box).

## Severity

- 🔴 **Important**: could launch a second live task, recreate the NAT instance, leak a key or a cross-tenant row, corrupt or misread a row, ship an unscoped `terraform apply`, leave a deployed binary reading an env it no longer gets, or turn CI red. Fixed before merge.
- 🟡 **Nit**: naming, wording, a comment, a doc phrasing, a refactor that would be nicer. At most five per review; the rest as a count in the summary.
- 🟣 **Pre-existing**: a real bug the diff did not introduce. Reported once, then filed as an Intent issue labelled `bug` and `source:agent`, not fixed in this PR; with `agent:fix` and the merge-when-green tier when it is self-evident (docs/conventions.md § Issues).

## Do not report

- Anything the verify gate already decides: fmt, `clippy -D warnings`, tests, `terraform fmt` / `validate`, workflow YAML parse, the knowledge budgets, the guard-hook self-test.
- `Cargo.lock`, `site/package-lock.json`, `site/data/`, `target/`.
- Test code that breaks a production rule on purpose (a mock repository, a fixture row).
- The rollout order of a `terraform/**` change: the `pbtb-deploy` skill owns it and the PR's Rollout section states it.

## Verification bar

A finding cites `file:line`. A bug finding names the input and the wrong result. A security finding names the sink the value reaches. A terraform finding names the resource and the attribute. Behaviour inferred from a name rather than read from the code is not a finding. When unsure, lower the confidence rather than raise the count.

## Re-review and summary

After the first review of a PR, report Important findings only. The summary opens with a tally in the form `N important, M nit, K pre-existing`, or `no important findings` when N is 0; the body lists findings by severity, each with its fix.
