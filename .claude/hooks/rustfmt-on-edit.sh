#!/usr/bin/env bash
# PostToolUse hook: format the just-edited Rust file with rustfmt.
#
# Reads the tool-call JSON on stdin and formats tool_input.file_path when it is
# a .rs file. Edition is taken from rustfmt.toml at the repo root, so a bare
# `rustfmt <file>` matches `cargo fmt` instead of failing E0670 on 2024 syntax.
#
# Extraction uses sed (not jq) because jq is not present on every host (e.g. the
# Windows Git Bash that runs these hooks). Never blocks the edit: any failure or
# missing rustfmt exits 0.

f=$(sed -n 's/.*"file_path"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -n1)

# Windows paths arrive JSON-escaped (E:\\projects\\...); collapse \\ back to \.
f=${f//\\\\/\\}

case "$f" in
  *.rs) command -v rustfmt >/dev/null 2>&1 && rustfmt "$f" >/dev/null 2>&1 ;;
esac

exit 0
