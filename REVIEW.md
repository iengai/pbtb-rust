# Review instructions

Policy for every review here, by a reviewer agent in `.claude/agents/`, Claude Code Review or a person. `pbtb-ship` runs them before a PR opens; the PR's Review section carries the tally.

## Passes, in order

1. **Bugs.** Logic errors, broken edge cases, regressions. Where they hide, each with its slug: a DynamoDB condition expression or projection that no longer matches the row shape `row-shape`; the start lock claimed after `RunTask` or not at all `launch-no-lock`; desired state (`Bot.enabled`) read as observed state (`BotRuntime`) `desired-as-observed`; an error flattened to a string so its retryability is lost `error-flattened`; a Telegram dialogue state no callback can reach `dead-dialogue`; a workflow step whose failure is swallowed `swallowed-step`; an ops-script whitelist that admits a shell metacharacter or a path outside the checkout `whitelist-escape`.
2. **Security.** `user_id` from client input, not the authenticated principal `user-id-from-client`; `api_key` / `secret_key`, a token or a model key reaching a log, an error, a `Debug` impl, a projection, an artifact or a comment `secret-sink`; a launch path without the start lock `launch-no-lock`; a public route (web API, Pages data) returning a bot config or its description `config-public`; a terraform policy wider than the role's stated purpose `policy-wide`; a workflow with more `permissions` than it uses, or an issue/comment trigger without an `author_association` gate `workflow-perms`.
3. **Compliance.** The diff does what the issue's *Done when* and the PR's **What** say, no more `scope`; every AGENTS.md invariant the change comes near is respected `invariant`; the layering holds (`architecture-reviewer`) `layering`; new comments survive without the diff (`comment-reviewer`) `comment-narration`; the docs leaf, skill, or invariant that described the old behaviour is updated `stale-doc`.

## Severity

- 🔴 **Important**: could launch a second live task, recreate the NAT instance, leak a key or a cross-tenant row, corrupt or misread a row, ship an unscoped `terraform apply`, leave a deployed binary reading an env it no longer gets, or turn CI red. Fixed before merge.
- 🟡 **Nit**: naming, wording, a comment, a doc phrasing, a nicer refactor. At most five per review; the rest as a count.
- 🟣 **Pre-existing**: a real bug the diff did not introduce. Reported once, filed as an Intent issue labelled `bug` and `source:agent`, not fixed here; with `agent:fix` and the merge-when-green tier when it is self-evident.

## Finding lines and repeats

A finding is one line: `- [cat:<slug>] Important|Nit <file:line> — <finding>`, a slug from the passes or `other`. A slug already in a merged PR body (`gh pr list --state merged --limit 500 --json number,body`) for the same component (the cited path's directory; a security slug or a Lesson line on the slug alone) is a **REPEAT**: it keeps its severity, escapes the nit cap; its fix includes the correction to the layer docs/conventions.md § Knowledge placement names, stated as a rule, not history. `other` never repeats.

## Do not report

- Anything the verify gate decides (fmt, clippy, tests, terraform checks, workflow YAML, budgets, the guard self-test).
- `Cargo.lock`, `site/package-lock.json`, `site/data/`, `target/`.
- Test code that breaks a production rule on purpose (a mock, a fixture row).
- The rollout order of a `terraform/**` change: `pbtb-deploy` owns it, the PR's Rollout section states it.

## Verification bar

A finding cites `file:line`. A bug finding names the input and the wrong result; a security finding the sink the value reaches; a terraform finding the resource and the attribute. Behaviour inferred from a name, not read from the code, is not a finding. When unsure, lower the confidence, not raise the count.

## Re-review and summary

After the first review of a PR, report Important findings only. The summary opens with the tally `N important, M nit, K pre-existing` (`no important findings` when N is 0); the body lists findings by severity, each with its fix.
