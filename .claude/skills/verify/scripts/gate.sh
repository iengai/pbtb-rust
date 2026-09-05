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

container_up() { docker exec app-node true >/dev/null 2>&1; }
if [ "$MODE" = auto ]; then
  if container_up; then MODE=container; else MODE=host; echo "(docker/app-node not reachable -> host toolchain; re-run --container before merging)"; fi
fi
if [ "$MODE" = container ] && ! container_up; then echo "app-node container is not running"; exit 2; fi

run_cargo() { # runs a cargo command in the chosen toolchain, returns its exit code
  if [ "$MODE" = container ]; then
    MSYS_NO_PATHCONV=1 docker exec app-node bash -lc "cd /app && $*" ; return $?
  else
    bash -lc "$*" ; return $?
  fi
}

echo "== toolchain: $MODE =="

# 1. fmt (always on the host: pure formatter, same rustfmt.toml)
cargo fmt --check >/dev/null 2>&1; gate fmt $?

# 2. check incl. test targets
OUT=$(run_cargo cargo check --workspace --all-targets 2>&1); RC=$?
gate check-all-targets $RC; [ $RC -ne 0 ] && echo "$OUT" | grep -E "^error" -A6 | head -40

# 3. clippy gate
OUT=$(run_cargo cargo clippy --workspace --all-targets --all-features -- -D warnings 2>&1); RC=$?
gate clippy-D-warnings $RC; [ $RC -ne 0 ] && echo "$OUT" | grep -E "^error" -A8 | head -60

# 4. tests
OUT=$(run_cargo cargo test --workspace 2>&1); RC=$?
if echo "$OUT" | grep -qE "FAILED|panicked"; then RC=1; fi
gate tests $RC
echo "$OUT" | grep -E "test result:" | sed 's/^/  /'
[ $RC -ne 0 ] && echo "$OUT" | grep -E "FAILED|panicked" -B3 | head -40
if [ "$MODE" = host ]; then echo "  (host: dynamodb-local integration tests may have self-skipped)"; fi

# 5. terraform, only when touched
CHANGED=$(git diff --name-only origin/main...HEAD 2>/dev/null; git diff --name-only; git diff --name-only --cached)
if echo "$CHANGED" | grep -q '^terraform/'; then
  terraform fmt -check -recursive terraform/ >/dev/null 2>&1; gate terraform-fmt $?
  AWS_PROFILE="${AWS_PROFILE:-dev}" terraform -chdir=terraform/envs/dev validate >/dev/null 2>&1; gate terraform-validate $?
  echo "  (state moves / env changes: also run a read-only targeted plan and quote 'Plan: … to destroy')"
fi

# 6. workflows, only when touched
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

echo "== $([ $FAILS -eq 0 ] && echo ALL GATES GREEN || echo "$FAILS GATE(S) FAILED") =="
exit $FAILS
