#!/usr/bin/env python3
"""PreToolUse hook: refuse the shell commands an agent may never run here.

Reads the tool-call JSON on stdin (tool_name, tool_input.command) and answers
with a `deny` decision when the command matches a rule below; anything else is
allowed without a word. The rules are the mechanical half of the invariants in
AGENTS.md: a rule that a script can decide lives here so no session has to
remember it.

  terraform destroy, or apply -destroy         never from an agent
  terraform apply without -target              the NAT is the sole trading egress;
                                               an unscoped apply can rebuild it
  aws ecs run-task / stop-task                 launching or stopping a live
                                               trading task is a human's call

The match runs on the whole command string, so a command wrapped in
`docker exec … bash -lc '…'` or chained after `&&` is caught the same way.
Malformed input is allowed through: this hook guards against known commands,
it is not a parser of every shell.

  python .claude/hooks/guard-shell.py --self-test     runs the cases below
"""
from __future__ import annotations

import json
import re
import sys

TERRAFORM = r"\bterraform\b(?:\s+-\S+)*\s+"
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
]


def verdict(command: str) -> str | None:
    """The refusal reason for a command, or None when it may run."""
    flat = " ".join(command.split())
    for pattern, reason in RULES:
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
    reason = verdict(command)
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


CASES: list[tuple[str, bool]] = [
    # (command, allowed)
    ("terraform plan -target=module.lambda_mcp_http", True),
    ("AWS_PROFILE=dev terraform -chdir=terraform/envs/dev plan -input=false -no-color", True),
    ("terraform apply -target=module.lambda_task_state_change_handler -target=aws_ssm_parameter.telebot_base_env", True),
    ("terraform apply -input=false -target module.chart_bucket", True),
    ("terraform -chdir=terraform/envs/dev apply -auto-approve -target=module.network.aws_instance.nat", True),
    ("terraform import 'module.x.aws_cloudwatch_log_group.this' /aws/lambda/x", True),
    ("terraform apply", False),
    ("cd terraform/envs/dev && terraform apply -auto-approve", False),
    ("AWS_PROFILE=dev terraform -chdir=terraform/envs/dev apply -input=false", False),
    ("terraform apply -target=module.x -destroy", False),
    ("terraform destroy -target=module.x", False),
    ("MSYS_NO_PATHCONV=1 docker exec app-node bash -lc 'cd /app && terraform apply'", False),
    ("terraform apply -replace=module.lambda_x.module.base.aws_lambda_function.this", False),
    ("aws ecs describe-tasks --cluster c --tasks t", True),
    ("aws ecs list-tasks --cluster c", True),
    ("python scripts/ops/pbtb_ops.py bot-status all --memory", True),
    ("aws ecs run-task --cluster c --task-definition td", False),
    ("PYTHONUTF8=1 aws --profile dev ecs stop-task --cluster c --task t", False),
    ("aws lambda invoke --function-name f out.json", True),
    ("echo 'terraform applying is fun'", True),
    # The words inside a quoted argument are not told apart from a command;
    # search with a pattern that does not spell the command out.
    ("grep -rn 'terraform apply' docs/", False),
    ("grep -rn 'terraform appl' docs/", True),
]


def self_test() -> int:
    bad = 0
    for command, allowed in CASES:
        got = verdict(command) is None
        if got != allowed:
            bad += 1
            print(f"FAIL {'allowed' if got else 'denied'}, expected {'allowed' if allowed else 'denied'}: {command}")
    print(f"{len(CASES) - bad}/{len(CASES)} cases pass")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(self_test() if "--self-test" in sys.argv[1:] else main())
