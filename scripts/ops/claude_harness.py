"""Claude Code headless, bounded by a hook, for the agents that run in CI.

The fix agent (fix_issue.py) and the diagnose agent (diagnose_issue.py)
drive `claude -p` on a checkout of this repository, so the model works with
the harness the repository is written for: CLAUDE.md and AGENTS.md, the
skills, the agents and the hooks under .claude/. The model behind it is
whatever ANTHROPIC_BASE_URL serves, by default DeepSeek's Anthropic-
compatible endpoint and its `deepseek-flash`; every alias the harness may
pick on its own resolves to that one model.

What the model may do is decided by a hook each script provides
(`<script> hook`, wired through the settings file `write_settings` makes):
a Bash command outside the script's whitelist is denied, a Read / Grep /
Glob of a path outside the checkout is denied, an edit is denied unless the
script allows edits and the path is not human-owned. The same hook records
every Bash command with its exit code and output to HARNESS_RUNS_LOG, the
model key blanked, which is the trail the scripts read and print.

The model key stays in the harness's environment, since Claude Code reads
it there; a command the model runs is that process's child and can read
the key. Everything that leaves a run (the command log, the answer, the
trail) goes through `masked()`.
"""
from __future__ import annotations

import json
import os
import re
import shlex
import subprocess
import sys
from collections.abc import Callable
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
CLAUDE = os.environ.get("CLAUDE_BIN", "claude")
DEFAULT_BASE_URL = "https://api.deepseek.com/anthropic"
DEFAULT_MODEL = "deepseek-flash"
TOOL_OUTPUT_CAP = 12_000

# The harness tools the hook judges by path, and the ones no agent gets:
# nothing leaves the checkout, nothing delegates, nothing plans aside.
READ_TOOLS = ("Read", "Grep", "Glob")
EDIT_TOOLS = ("Edit", "Write", "MultiEdit", "NotebookEdit")
DISALLOWED_TOOLS = ("WebFetch", "WebSearch", "Task", "Agent", "TodoWrite", "NotebookEdit")

# The read-only git family both agents get: plain options, quoted values
# (no expansion, redirection or chaining characters inside), revisions and
# paths. `--output` writes a file wherever it is
# told, `--contents` makes blame print any file as uncommitted lines, and a
# `..` path segment leaves the checkout; a `..` inside a revision range
# (`origin/main..HEAD`) is not one.
QUOTED = r"'[^'`$|&;<>\n]*'|\"[^\"`$|&;<>\n\\]*\""
GIT_READ = re.compile(r"^git (log|show|blame)( (--|--?[A-Za-z0-9-]+(=([A-Za-z0-9_:./,%-]+|" + QUOTED + r"))?|[A-Za-z0-9_:./^~,-]+|" + QUOTED + r"))*$")
ESCAPES = re.compile(r"(^|\s)--(output|contents)\b|(^|[\s='\"/])\.\.(/|\s|$)")


def clip(text: str, cap: int = TOOL_OUTPUT_CAP) -> str:
    return text if len(text) <= cap else text[:cap] + f"\n… [{len(text) - cap} more chars clipped]"


def gh(*args: str) -> str:
    return subprocess.run(["gh", *args], check=True, capture_output=True, text=True, encoding="utf-8").stdout


def masked(text: str) -> str:
    """The text with every secret of the environment blanked: a command can print its environment, and what it prints is kept."""
    for k in ("ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN", "AWS_SECRET_ACCESS_KEY", "AWS_SESSION_TOKEN", "SENTRY_AUTH_TOKEN"):
        v = os.environ.get(k)
        if v and len(v) >= 8:
            text = text.replace(v, "***")
    return text


def model_env(runs_log: Path, home: Path, *, keep_aws: bool = False, bash_timeout_s: int = 900) -> dict[str, str]:
    """The environment Claude Code runs in.

    The GitHub token never enters it. AWS credentials enter only for an agent
    whose whitelisted commands need them (the diagnose agent's ops probes,
    under a read-only role). The model key stays, since Claude Code reads it
    from ANTHROPIC_API_KEY. Every model alias the harness may pick on its own
    (a fast model for a summary, a subagent) resolves to the one configured
    model, so nothing reaches another model on the endpoint by a name this
    script never chose.
    """
    drop = ("GH_", "GITHUB_TOKEN") + (() if keep_aws else ("AWS_",))
    env = {k: v for k, v in os.environ.items() if not k.startswith(drop)}
    env.setdefault("ANTHROPIC_BASE_URL", DEFAULT_BASE_URL)
    # The repository variable behind ANTHROPIC_MODEL may hold a comma-separated
    # list; the first entry is the model.
    model = (env.get("ANTHROPIC_MODEL") or DEFAULT_MODEL).split(",")[0].strip() or DEFAULT_MODEL
    env["ANTHROPIC_MODEL"] = model
    for k in ("ANTHROPIC_DEFAULT_HAIKU_MODEL", "ANTHROPIC_DEFAULT_SONNET_MODEL", "ANTHROPIC_DEFAULT_OPUS_MODEL",
              "ANTHROPIC_SMALL_FAST_MODEL", "CLAUDE_CODE_SUBAGENT_MODEL"):
        env.setdefault(k, model)
    env.update(
        CLAUDE_CONFIG_DIR=str(home), HARNESS_RUNS_LOG=str(runs_log),
        CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC="1", DISABLE_AUTOUPDATER="1", DISABLE_TELEMETRY="1",
        BASH_DEFAULT_TIMEOUT_MS=str(bash_timeout_s * 1000), BASH_MAX_TIMEOUT_MS=str(bash_timeout_s * 1000),
        BASH_MAX_OUTPUT_LENGTH=str(TOOL_OUTPUT_CAP),
        CARGO_TERM_COLOR="never", PYTHONUTF8="1", PYTHONIOENCODING="utf-8",
    )
    return env


def deny(reason: str) -> dict:
    return {"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "deny",
                                   "permissionDecisionReason": reason}}


def inside(raw: str) -> str | None:
    """The checkout-relative posix path of `raw`, or None when it resolves outside the checkout."""
    try:
        return (ROOT / raw).resolve().relative_to(ROOT).as_posix()
    except ValueError:
        return None


def hook(payload: dict, *, allowed: Callable[[str], bool], note: str,
         denied: Callable[[str], bool] | None = None) -> dict | None:
    """The PreToolUse / PostToolUse hook Claude Code calls for the model's tools.

    Returns the hook's answer (a denial) or None for silence. Before a tool
    runs: a Bash command `allowed` refuses, a Read / Grep / Glob of a path
    outside the checkout, an edit when the script allows none (`denied` is
    None) or of a path `denied` names or outside the checkout, are denied.
    After a Bash command: the command, its exit code and its output go to
    HARNESS_RUNS_LOG. The first call also leaves a marker beside the log, so
    a run whose hooks never fired can be told apart from a run that never
    ran a command.
    """
    event, tool, inp = payload.get("hook_event_name"), payload.get("tool_name"), payload.get("tool_input") or {}
    log = os.environ.get("HARNESS_RUNS_LOG")
    if log:
        Path(log + ".hooked").touch()
    if event == "PreToolUse":
        if tool == "Bash":
            if not allowed(inp.get("command") or ""):
                return deny(f"command not allowed here; allowed: {note}")
        elif tool in READ_TOOLS:
            raw = inp.get("file_path") or inp.get("path") or ""
            if raw and inside(raw) is None:
                return deny(f"{raw} is outside the checkout; only the checkout is readable here")
        elif tool in EDIT_TOOLS:
            if denied is None:
                return deny("this run is read-only; nothing is edited here")
            raw = inp.get("file_path") or inp.get("notebook_path") or ""
            rel = inside(raw)
            if rel is None:
                return deny(f"{raw} is outside the checkout")
            if denied(rel):
                return deny(f"{rel} is human-owned (see the deny list); say so in your answer instead")
    elif event == "PostToolUse" and tool == "Bash" and log:
        resp = payload.get("tool_response")
        if isinstance(resp, dict):
            rc = resp.get("exit_code")
            out = f"{resp.get('stdout') or ''}\n{resp.get('stderr') or ''}"
        else:
            rc, out = None, str(resp or "")
        # The harness reports a failing command's status as a line of text in
        # the output rather than a field; a payload with neither is left as
        # rc None, which the reader takes from the output.
        m = rc is None and re.search(r"^Exit code:? (\d+)\s*$", out, re.M)
        if m:
            rc = int(m.group(1))
        row = {"cmd": masked(inp.get("command") or ""), "rc": rc, "out": masked(out)[-TOOL_OUTPUT_CAP:]}
        if rc is None and isinstance(resp, dict):
            row["response_keys"] = sorted(resp)
        with open(log, "a", encoding="utf-8") as f:
            f.write(json.dumps(row) + "\n")
    return None


def hook_main(decide: Callable[[dict], dict | None], agent: str) -> int:
    """The `hook` subcommand: JSON on stdin, a decision on stdout.

    A hook that cannot decide must not let the tool run: an exception
    anywhere in the check is a denial, not a pass.
    """
    try:
        answer = decide(json.load(sys.stdin))
    except Exception as e:  # noqa: BLE001
        answer = deny(f"the {agent} agent's hook failed: {e!r}")
    if answer:
        print(json.dumps(answer))
    return 0


def write_settings(path: Path, script: Path, *, bash: tuple[str, ...], edits: bool,
                   deny_globs: tuple[str, ...] | list[str] = ()) -> Path:
    """The settings Claude Code runs under: the hook on every tool that reads or acts, and the permission rules.

    The hook is the gate that is tested offline; the deny rules repeat the
    script's deny list in the harness's own glob syntax as a second layer.
    Bash is allowed by family so that a command never prompts (`dontAsk`
    mode denies whatever would): the hook decides the exact command.
    """
    hook_cmd = f"{shlex.quote(sys.executable)} {shlex.quote(str(script.resolve()))} hook"
    allow = list(READ_TOOLS) + ["Skill"] + [f"Bash({b} *)" for b in bash]
    if edits:
        allow += ["Edit", "Write", "MultiEdit"]
        deny_rules = [f"{tool}({pat.replace('*', '**')})" for pat in deny_globs for tool in ("Edit", "Write", "MultiEdit")]
    else:
        deny_rules = ["Edit", "Write", "MultiEdit", "NotebookEdit"]
    settings = {
        "permissions": {"allow": allow, "deny": deny_rules, "defaultMode": "dontAsk"},
        "hooks": {
            "PreToolUse": [{"matcher": "|".join(("Bash",) + READ_TOOLS + EDIT_TOOLS),
                            "hooks": [{"type": "command", "command": hook_cmd}]}],
            "PostToolUse": [{"matcher": "Bash", "hooks": [{"type": "command", "command": hook_cmd}]}],
        },
    }
    path.write_text(json.dumps(settings, indent=1), encoding="utf-8")
    return path


def claude_print(prompt: str, *, settings: Path, env: dict, max_turns: int, timeout: float, system: str = "",
                 allowed: tuple[str, ...] = (), disallowed: tuple[str, ...] = (), resume: str = "") -> dict:
    """One headless Claude Code run; the parsed `--output-format json` result, or an error dict."""
    cmd = [CLAUDE, "-p", "--output-format", "json", "--permission-mode", "dontAsk",
           "--settings", str(settings), "--max-turns", str(max_turns)]
    if resume:
        cmd += ["--resume", resume]
    if system:
        cmd += ["--append-system-prompt", system]
    if allowed:
        cmd += ["--allowedTools", *allowed]
    if disallowed:
        cmd += ["--disallowedTools", *disallowed]
    try:
        r = subprocess.run(cmd, input=prompt, cwd=ROOT, capture_output=True, text=True, encoding="utf-8",
                           errors="replace", timeout=timeout, env=env)
    except subprocess.TimeoutExpired:
        return {"is_error": True, "result": "", "error": f"claude did not finish within {int(timeout)}s"}
    except FileNotFoundError:
        return {"is_error": True, "result": "", "error": f"{CLAUDE} is not installed"}
    text = r.stdout.strip()
    data: object = {}
    if "{" in text:
        try:
            data = json.loads(text[text.index("{"):])
        except json.JSONDecodeError:
            data = {}
    # A run that ended on its turn budget or an execution error reports
    # `type: result` with a subtype and no `result` text at all.
    if not isinstance(data, dict) or not ("result" in data or data.get("type") == "result"):
        return {"is_error": True, "result": "",
                "error": (r.stderr.strip() or text)[-1500:] or f"claude exited {r.returncode} with no JSON"}
    data.setdefault("result", "")
    if (r.returncode or data.get("subtype", "success") != "success") and not data.get("is_error"):
        data["is_error"] = True
    data.setdefault("error", r.stderr.strip()[-1500:] or str(data.get("subtype", "")))
    return data


def run_agent(prompt: str, *, settings: Path, env: dict, system: str, max_turns: int, timeout: float,
              disallowed: tuple[str, ...], printer: Callable[..., dict] = claude_print) -> tuple[str, list[str]]:
    """One agent run: the answer and its trail, both with the model key blanked.

    A run that spends its turn budget without an answer is resumed once,
    with every tool disallowed, for the answer the shape asks for and a note
    of what is unfinished. `printer` is the harness call, injectable for an
    offline test.
    """
    data = printer(prompt, settings=settings, env=env, max_turns=max_turns, timeout=timeout,
                   system=system, disallowed=disallowed)
    trail = [f"{data.get('num_turns', '?')} turns, {int(data.get('duration_ms') or 0) // 1000}s"]
    if data.get("subtype") == "error_max_turns" and data.get("session_id") and not (data.get("result") or "").strip():
        again = printer("The tool budget is spent. Answer now in the required shape and name what is unfinished.",
                        settings=settings, env=env, max_turns=1, timeout=min(timeout, 600), resume=data["session_id"],
                        system=system, disallowed=("Bash", "Skill") + READ_TOOLS + EDIT_TOOLS + DISALLOWED_TOOLS)
        trail.append("turn budget spent; resumed once, without tools, for the answer")
        data = {**again, "permission_denials": data.get("permission_denials") or []}
    trail += [f"denied {n.get('tool_name')}({json.dumps(n.get('tool_input'))[:160]})"
              for n in data.get("permission_denials") or []]
    answer = (data.get("result") or "").strip()
    if data.get("is_error"):
        trail.append(f"harness error: {(data.get('error') or '')[:300]}")
        if not answer:
            answer = "The harness ended without an answer: " + (data.get("error") or "")[:500]
    # Everything here is text the model wrote or saw, and it goes to a public
    # comment or artifact.
    return masked(answer), [masked(t) for t in trail]


def read_runs(runs_log: Path) -> list[dict]:
    rows = []
    if runs_log.is_file():
        for line in runs_log.read_text(encoding="utf-8", errors="replace").splitlines():
            try:
                rows.append(json.loads(line))
            except json.JSONDecodeError:
                continue
    return rows


def prepare(out: Path, script: Path, *, bash: tuple[str, ...], edits: bool, deny_globs=(),
            keep_aws: bool = False, bash_timeout_s: int = 900) -> tuple[Path, Path, dict[str, str]]:
    """The run directory made ready: (runs_log, settings, env). `out` is what the workflow uploads."""
    if ROOT in out.parents or out == ROOT:
        sys.exit("the run directory must lie outside the checkout")
    out.mkdir(parents=True, exist_ok=True)
    runs_log = out / "runs.jsonl"
    for stale in (runs_log, Path(str(runs_log) + ".hooked")):
        stale.unlink(missing_ok=True)
    # The harness's home holds its session transcripts, every tool output
    # included; it stays outside the directory the workflow uploads.
    home = out.parent / f"{out.name}-claude-home"
    home.mkdir(exist_ok=True)
    settings = write_settings(out / "claude-settings.json", script, bash=bash, edits=edits, deny_globs=deny_globs)
    return runs_log, settings, model_env(runs_log, home, keep_aws=keep_aws, bash_timeout_s=bash_timeout_s)


def logged_note(runs_log: Path) -> str:
    rows = read_runs(runs_log)
    return f"{len(rows)} commands logged" + ("" if Path(str(runs_log) + ".hooked").exists() else "; the hook never fired")
