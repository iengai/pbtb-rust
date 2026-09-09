@../AGENTS.md

# Claude Code notes

Behaviours of the Claude Code harness on this Windows host. They are not project rules; other agents can skip this file.

- Bash-tool heredocs collapse `\\` and choke on Rust `\u{…}` escapes. Write any edit script that contains backslashes, unicode escapes, or more than a screen of text with the Write tool, then run `python <file>`.
- `set -e` does not stop a multi-step Bash-tool command. Guard every gate with `|| exit 1` and check `${PIPESTATUS[0]}` after a pipe.
- The console is cp932. Set `PYTHONIOENCODING=utf-8` (and `PYTHONUTF8=1` for the AWS CLI) before printing non-ASCII, or the output dies mid-stream and reads as corrupt JSON.
- Git Bash rewrites absolute paths in arguments: `docker exec -w /app` fails, so use `bash -lc 'cd /app && …'` and prefix `MSYS_NO_PATHCONV=1`.
