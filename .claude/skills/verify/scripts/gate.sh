#!/usr/bin/env bash
# Verification gate for pbtb-rust. Every step is guarded explicitly: in this
# harness `set -e` does not stop a multi-step command, and a pipeline hides the
# exit code of its first stage. Prints one `GATE <name>: ok|FAIL` line per gate
# and exits non-zero if any failed.
#
#   bash .claude/skills/verify/scripts/gate.sh            # auto: container if up, else host
#   bash .claude/skills/verify/scripts/gate.sh --host     # host toolchain only
#   bash .claude/skills/verify/scripts/gate.sh --container
set -u
MODE=auto
for arg in "$@"; do case "$arg" in --host) MODE=host;; --container) MODE=container;; esac; done

cd "$(git rev-parse --show-toplevel)" || exit 2
FAILS=0
gate() { # name, exit code
  if [ "$2" -eq 0 ]; then echo "GATE $1: ok"; else echo "GATE $1: FAIL"; FAILS=$((FAILS+1)); fi
}

# Print why a gate failed. The matching lines when there are any, and the tail
# regardless: a tool that dies before it can emit a diagnostic (a missing
# component, an unreadable manifest) produces no match, and a FAIL with no
# output at all is a gate nobody can act on.
show() { # output, [grep pattern]
  local pattern="${2:-^error}"
  echo "$1" | grep -E "$pattern" -A8 | head -60
  echo "  --- last 20 lines ---"
  echo "$1" | tail -20 | sed 's/^/  /'
}

container_up() { docker exec app-node true >/dev/null 2>&1; }
if [ "$MODE" = auto ]; then
  if container_up; then MODE=container; else MODE=host; echo "(docker/app-node not reachable -> host toolchain; re-run --container before merging)"; fi
fi
if [ "$MODE" = container ] && ! container_up; then echo "app-node container is not running"; exit 2; fi

# The container mounts the MAIN checkout at /app. A worktree under
# .claude/worktrees/<name> is visible there as /app/.claude/worktrees/<name> and
# gets a target dir of its own, so two sessions' builds do not overwrite each
# other's test binaries. A checkout outside the main root is not visible in the
# container at all, and the cargo gates would silently verify someone else's tree.
CDIR=/app; CTARGET=/app/target
if [ "$MODE" = container ]; then
  MAIN_ROOT=$(dirname "$(git rev-parse --path-format=absolute --git-common-dir)")
  TOP=$(git rev-parse --show-toplevel)
  if [ "$TOP" != "$MAIN_ROOT" ]; then
    REL=${TOP#"$MAIN_ROOT"/}
    if [ "$REL" = "$TOP" ]; then
      echo "this checkout ($TOP) is outside the container's /app mount: work under .claude/worktrees/ or run --host"; exit 2
    fi
    CDIR="/app/$REL"; CTARGET="/app/target/worktrees/$(basename "$TOP")"
  fi
fi

run_cargo() { # runs a cargo command in the chosen toolchain, returns its exit code
  if [ "$MODE" = container ]; then
    MSYS_NO_PATHCONV=1 docker exec -e CARGO_TERM_COLOR=never -e CARGO_TARGET_DIR="$CTARGET" app-node bash -lc "cd '$CDIR' && $*" ; return $?
  else
    CARGO_TERM_COLOR=never bash -lc "$*" ; return $?
  fi
}

echo "== toolchain: $MODE$([ "$CDIR" != /app ] && echo " ($CDIR, target $CTARGET)") =="

# 1. fmt (always on the host: pure formatter, same rustfmt.toml)
CARGO_TERM_COLOR=never cargo fmt --check >/dev/null 2>&1; gate fmt $?

# 2. check incl. test targets
OUT=$(run_cargo cargo check --workspace --all-targets 2>&1); RC=$?
gate check-all-targets $RC; [ $RC -ne 0 ] && show "$OUT"

# 3. clippy gate
OUT=$(run_cargo cargo clippy --workspace --all-targets --all-features -- -D warnings 2>&1); RC=$?
gate clippy-D-warnings $RC; [ $RC -ne 0 ] && show "$OUT"

# 4. tests
OUT=$(run_cargo cargo test --workspace 2>&1); RC=$?
if echo "$OUT" | grep -qE "FAILED|panicked"; then RC=1; fi
gate tests $RC
echo "$OUT" | grep -E "test result:" | sed 's/^/  /'
[ $RC -ne 0 ] && show "$OUT" "FAILED|panicked"
# The dynamodb-local suites skip themselves when no server is reachable, and a
# skip reads as a pass. app-node has no docker socket, so in the container they
# depend on APP__DYNAMODB__ENDPOINT_URL pointing at the compose service.
if [ "$MODE" = container ]; then
  docker exec app-node bash -lc '[ -n "${APP__DYNAMODB__ENDPOINT_URL:-}" ]' >/dev/null 2>&1     || echo "  (app-node has no APP__DYNAMODB__ENDPOINT_URL: the dynamodb-local suites just self-skipped)"
else
  echo "  (host: dynamodb-local suites self-skip unless Docker or APP__DYNAMODB__ENDPOINT_URL is available)"
fi

# 5. terraform, only when touched
CHANGED=$(git diff --name-only origin/main...HEAD 2>/dev/null; git diff --name-only; git diff --name-only --cached)
if echo "$CHANGED" | grep -q '^terraform/'; then
  terraform fmt -check -recursive terraform/ >/dev/null 2>&1; gate terraform-fmt $?
  AWS_PROFILE="${AWS_PROFILE:-dev}" terraform -chdir=terraform/envs/dev validate >/dev/null 2>&1; gate terraform-validate $?
  echo "  (state moves / env changes: also run a read-only targeted plan and quote 'Plan: … to destroy')"
fi

# 6. the web console, only when it exists (site/package.json) — lint (tsc +
# eslint) and the production build, which is what pages-publish runs.
if [ -f site/package.json ]; then
  if [ ! -d site/node_modules ]; then
    OUT=$(cd site && npm ci 2>&1); RC=$?
    [ $RC -ne 0 ] && { gate site-npm-ci $RC; show "$OUT" "ERR!|error"; }
  fi
  if [ -d site/node_modules ]; then
    OUT=$(cd site && npm run lint 2>&1); RC=$?
    gate site-lint $RC; [ $RC -ne 0 ] && show "$OUT" "error|✖"
    OUT=$(cd site && npm run build 2>&1); RC=$?
    gate site-build $RC; [ $RC -ne 0 ] && show "$OUT" "error|Error"
  fi
fi

# 7. workflows, only when touched
if echo "$CHANGED" | grep -q '^\.github/workflows/'; then
  RC=0
  for f in $(echo "$CHANGED" | grep '^\.github/workflows/.*\.ya\?ml$' | sort -u); do
    [ -f "$f" ] || continue
    python - "$f" <<'PY' || RC=1
import sys
try:
    import yaml
except ImportError:
    sys.exit(0)  # no pyyaml on this host: cannot check, do not fail
yaml.safe_load(open(sys.argv[1], encoding="utf-8"))
PY
  done
  gate workflow-yaml $RC
fi

# 8. knowledge budgets (docs/conventions.md § Knowledge placement): the files
# every session loads and every skill description have a byte ceiling, and a
# change to the row layout or the infra is a prompt to re-read the leaf that
# describes it.
python - <<'PY'; gate knowledge-budget $?
import glob, os, re, sys
bad = 0
for f, lim in [("AGENTS.md", 6144), (".claude/CLAUDE.md", 1536)]:
    n = os.path.getsize(f)
    if n > lim:
        print(f"  {f}: {n} bytes > {lim}"); bad = 1
for f in glob.glob(".claude/skills/*/SKILL.md"):
    m = re.search(r"^description:[ \t]*(.*?)\n(?=\S)", open(f, encoding="utf-8").read(), re.S | re.M)
    n = len(m.group(1).encode("utf-8")) if m else 0
    if n > 300:
        print(f"  {f}: description {n} bytes > 300"); bad = 1
sys.exit(bad)
PY
echo "$CHANGED" | grep -q '^src/infra/botrepository\.rs$' && echo "  (botrepository.rs changed: does docs/data-model.md still describe the rows?)"
echo "$CHANGED" | grep -q '^terraform/' && echo "  (terraform changed: do docs/deployment/*.md and the pbtb-deploy skill still match?)"

echo "== $([ $FAILS -eq 0 ] && echo ALL GATES GREEN || echo "$FAILS GATE(S) FAILED") =="
exit $FAILS
