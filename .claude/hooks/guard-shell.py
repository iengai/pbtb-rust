#!/usr/bin/env python3
"""PreToolUse hook: refuse the shell commands an agent may never run here.

Reads the tool-call JSON on stdin (tool_name, tool_input.command) and answers
with a `deny` decision when the command matches a rule below; anything else is
allowed without a word. The rules are the mechanical half of the invariants in
AGENTS.md and of the local agent identity (docs/agents.md § Local agents): a
rule that a script can decide lives here so no session has to remember it.

  terraform destroy, or apply -destroy         never from an agent
  terraform apply without -target              the NAT is the sole trading egress;
                                               an unscoped apply can rebuild it
  aws ecs run-task / stop-task                 launching or stopping a live
                                               trading task is a human's call
  gh auth token|login|logout|switch|refresh|   the person's keyring, or a way to
    setup-git|git-credential, gh auth status   print or swap the agent's token
    --show-token, git credential, the agent's
    credential helper run by hand, a credential
    helper set with -c or git config
  gh secret, gh variable set|delete            the person's
  setting or unsetting GH_CONFIG_DIR,          reaches around the identity env
    GH_TOKEN, GITHUB_TOKEN, GIT_CONFIG_*,
    GIT_ASKPASS, or env -i / -u
  a path into ~/.config/pbtb-agent             the App key and its tokens
  gh, git push, git commit in a session        they would act as the person
    without the identity env

The match runs on the whole command string, so a command wrapped in
`docker exec … bash -lc '…'` or chained after `&&` is caught the same way.
Malformed input is allowed through: this hook guards against known commands,
it is not a parser of every shell. A script that runs `gh` itself is not
seen: with the identity env it acts as the App, and `nat_rebuild.py` refuses
to dispatch from a Claude Code session without it.

  python .claude/hooks/guard-shell.py --self-test     runs the cases below
"""
from __future__ import annotations

import json
import re
import sys
from pathlib import Path

TERRAFORM = r"\bterraform\b(?:\s+-\S+)*\s+"
GH = r"(?:^|[\s;&|('\"`])(?:[^\s'\"`]*[\\/])?gh(?:\.exe)?['\"`]?"
GIT = r"\bgit(?:\.exe)?(?:\s+-[Cc]\s+\S+)*"
IDENTITY_VARS = r"(?i:GH_CONFIG_DIR|GH_TOKEN|GITHUB_TOKEN|GH_ENTERPRISE_TOKEN|GIT_ASKPASS|GIT_CONFIG_\w+)"
RULES: list[tuple[re.Pattern[str], str]] = [
    (
        re.compile(TERRAFORM + r"destroy\b"),
        "terraform destroy is never run by an agent in this repo; hand the plan to a human.",
    ),
    (
        re.compile(TERRAFORM + r"apply\b(?=.*\s-destroy\b)"),
        "terraform apply -destroy is never run by an agent in this repo; hand the plan to a human.",
    ),
    (
        re.compile(TERRAFORM + r"apply\b(?!.*\s-target[= ])"),
        "terraform apply without -target can rebuild the NAT (the sole trading egress). "
        "Scope it: terraform apply -target=<address> …; see AGENTS.md § Terraform / NAT egress.",
    ),
    (
        re.compile(r"\baws\b(?:\s+\S+)*?\s+ecs\s+(run-task|stop-task)\b"),
        "aws ecs run-task / stop-task start or stop a live trading task; that stays with a human. "
        "Read state with describe-tasks / list-tasks or scripts/ops/pbtb_ops.py bot-status.",
    ),
    (
        re.compile(GH + r"\s+auth\s+(token|login|logout|switch|refresh|setup-git|git-credential)\b"
                   + r"|" + GH + r"\s+auth\s+status\b[^;&|]*\s(-t|--show-token)\b"
                   + r"|\bagent_identity\.py['\"]?\s+credential\b"
                   + r"|" + GIT + r"\s+credential(?![-\w])"
                   + r"|" + GIT + r"\s+(?:-c\s+|config\s+(?![^;&|]*--get)[^;&|]*?\s)credential\."),
        "Agent sessions act as the pbtb-local-agent App; the person's gh accounts and git credentials "
        "are not theirs to use, switch or print. `python scripts/ops/agent_identity.py status` shows the identity.",
    ),
    (
        re.compile(GH + r"\s+(secret\b|variable\s+(set|delete)\b)"),
        "Repository secrets and variables stay with the person (the App is not granted them); "
        "hand the exact command to the owner to run in their own terminal.",
    ),
    (
        re.compile(r"(?:^|[\s;&|(])(?:export\s+)?" + IDENTITY_VARS + r"="
                   + r"|(?i:\$env:)" + IDENTITY_VARS + r"\s*="
                   + r"|\bunset\s+(?:\S+\s+)*" + IDENTITY_VARS + r"\b"
                   + r"|\benv\s+(-i|-u|--unset|--ignore-environment)\b"
                   + r"|(?i:env):\\?" + IDENTITY_VARS + r"\b"),
        "The identity env (GH_CONFIG_DIR, git's credential helpers, the tokens) is set by "
        "scripts/ops/agent_identity.py for the whole session; commands do not override it.",
    ),
    (
        re.compile(r"\.config[\\/]+pbtb-agent\b|\bpbtb-agent[\\/]"),
        "~/.config/pbtb-agent holds the App key and its tokens; nothing reads it but "
        "scripts/ops/agent_identity.py (status, refresh).",
    ),
]
# Only while the session lacks the identity env: then these act as the person.
NO_IDENTITY_RULE = (
    re.compile(GH + r"(?=\s|$)|" + GIT + r"\s+(?:--?[\w-]+(?:=\S+)?\s+)*(?:push|commit)\b"),
    "This session has no agent identity, so gh / git push / git commit would act as the person. "
    "On a set-up machine run `python scripts/ops/agent_identity.py env` in this checkout and retry; "
    "the first time, `python scripts/ops/agent_identity.py app-url` (docs/agents.md § Local agents).",
)


def has_identity() -> bool:
    """Whether this session runs with the identity env; agent_identity.py owns the check."""
    sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "scripts" / "ops"))
    try:
        import agent_identity
    except ImportError:
        return False
    return agent_identity.session_has_identity()


def verdict(command: str, identity: bool = True) -> str | None:
    """The refusal reason for a command, or None when it may run."""
    flat = " ".join(command.split())
    rules = RULES if identity else RULES + [NO_IDENTITY_RULE]
    for pattern, reason in rules:
        if pattern.search(flat):
            return reason
    return None


def main() -> int:
    try:
        call = json.load(sys.stdin)
        command = call.get("tool_input", {}).get("command", "")
    except (ValueError, AttributeError):
        return 0
    if not isinstance(command, str):
        return 0
    reason = verdict(command, has_identity())
    if reason is None:
        return 0
    print(json.dumps({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "permissionDecision": "deny",
            "permissionDecisionReason": reason,
        }
    }))
    return 0


CASES: list[tuple[str, bool, bool]] = [
    # (command, allowed with the identity env, allowed without it)
    ("terraform plan -target=module.lambda_mcp_http", True, True),
    ("AWS_PROFILE=dev terraform -chdir=terraform/envs/dev plan -input=false -no-color", True, True),
    ("terraform apply -target=module.lambda_task_state_change_handler -target=aws_ssm_parameter.telebot_base_env", True, True),
    ("terraform apply -input=false -target module.chart_bucket", True, True),
    ("terraform -chdir=terraform/envs/dev apply -auto-approve -target=module.network.aws_instance.nat", True, True),
    ("terraform import 'module.x.aws_cloudwatch_log_group.this' /aws/lambda/x", True, True),
    ("terraform apply", False, False),
    ("cd terraform/envs/dev && terraform apply -auto-approve", False, False),
    ("AWS_PROFILE=dev terraform -chdir=terraform/envs/dev apply -input=false", False, False),
    ("terraform apply -target=module.x -destroy", False, False),
    ("terraform destroy -target=module.x", False, False),
    ("MSYS_NO_PATHCONV=1 docker exec app-node bash -lc 'cd /app && terraform apply'", False, False),
    ("terraform apply -replace=module.lambda_x.module.base.aws_lambda_function.this", False, False),
    ("aws ecs describe-tasks --cluster c --tasks t", True, True),
    ("aws ecs list-tasks --cluster c", True, True),
    ("python scripts/ops/pbtb_ops.py bot-status all --memory", True, True),
    ("aws ecs run-task --cluster c --task-definition td", False, False),
    ("PYTHONUTF8=1 aws --profile dev ecs stop-task --cluster c --task t", False, False),
    ("aws lambda invoke --function-name f out.json", True, True),
    ("echo 'terraform applying is fun'", True, True),
    # The words inside a quoted argument are not told apart from a command;
    # search with a pattern that does not spell the command out.
    ("grep -rn 'terraform apply' docs/", False, False),
    ("grep -rn 'terraform appl' docs/", True, True),
    # the local agent identity
    ("gh pr create --base main --head feat/x --title 'feat: x' --body-file b.md", True, False),
    ("gh pr view 12 --json author -q .author.login", True, False),
    ("cd site && npm run build && gh workflow run pages-publish.yml --ref main", True, False),
    ("'C:\\Program Files\\GitHub CLI\\gh.exe' pr list", True, False),
    ("git push -u origin feat/local-agent-identity", True, False),
    ("git -C ../other push --force-with-lease", True, False),
    ("git commit -F - <<'EOF'", True, False),
    ("git -c core.hooksPath=/dev/null commit -m 'x'", True, False),
    ("git status --short && git diff --stat", True, True),
    ("git log --oneline -5 origin/main", True, True),
    ("python scripts/ops/agent_identity.py status", True, True),
    ("python scripts/ops/nat_rebuild.py --dry-run", True, True),
    # A quoted `gh` is taken for a command, like the terraform cases above.
    ("grep -rn \"gh pr\" docs/", True, False),
    ("bash -c \"gh pr merge 1\"", True, False),
    ("bash -lc 'gh auth token'", False, False),
    ("echo `gh auth token`", False, False),
    ("gh auth status", True, False),
    ("gh auth token", False, False),
    ("gh auth switch --user iengai", False, False),
    ("gh auth setup-git", False, False),
    ("gh auth status --show-token", False, False),
    ("gh auth status -h github.com -t", False, False),
    ("printf 'protocol=https\\nhost=github.com\\n' | gh auth git-credential get", False, False),
    ("python scripts/ops/agent_identity.py credential get", False, False),
    ("git config --get credential.helper", True, True),
    ("git config user.email x && grep -n credential.helper docs/agents.md", True, True),
    ("printf 'protocol=https\\nhost=github.com\\n' | git credential fill", False, False),
    ("git -c credential.helper= push", False, False),
    ("git config --global credential.helper manager", False, False),
    ("git credential-manager --version", True, True),
    ("gh secret set SENTRY_AUTH_TOKEN --body x", False, False),
    ("gh variable set LOCAL_AGENT_USER_ID --body 1", False, False),
    ("gh variable list", True, False),
    ("GH_CONFIG_DIR= gh pr list", False, False),
    ("GH_TOKEN=abc gh api user", False, False),
    ("export GIT_CONFIG_COUNT=0", False, False),
    ("unset GH_CONFIG_DIR; gh pr list", False, False),
    ("env -u GH_CONFIG_DIR gh pr list", False, False),
    ("$env:GH_CONFIG_DIR = ''; gh pr list", False, False),
    ("Remove-Item Env:\\GH_CONFIG_DIR", False, False),
    ("$ENV:gh_config_dir = ''", False, False),
    ("cd ~/.config && cat pbtb-agent/private-key.pem", False, False),
    ("cat ~/.config/pbtb-agent/private-key.pem", False, False),
    ("Get-Content $HOME\\.config\\pbtb-agent\\gh\\hosts.yml", False, False),
    ("echo $GIT_AUTHOR_NAME", True, True),
]


def self_test() -> int:
    bad = 0
    total = 0
    for command, with_identity, without_identity in CASES:
        for identity, allowed in ((True, with_identity), (False, without_identity)):
            total += 1
            got = verdict(command, identity) is None
            if got != allowed:
                bad += 1
                print(f"FAIL {'allowed' if got else 'denied'}, expected {'allowed' if allowed else 'denied'} "
                      f"({'with' if identity else 'without'} identity): {command}")
    print(f"{total - bad}/{total} cases pass")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(self_test() if "--self-test" in sys.argv[1:] else main())
