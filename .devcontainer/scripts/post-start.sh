#!/usr/bin/env bash
set -Eeuo pipefail

echo "[devcontainer] post-start: begin"

cd /app

# Ensure the cargo cache + target volumes are writable by the current user. These are
# named volumes; if one is recreated while the container is only restarted (not
# recreated), it comes back root-owned and post-create.sh will NOT re-run. This guard
# is idempotent and only acts when a dir isn't writable, so it is a no-op on the normal
# path (no per-start cost once ownership is correct).
for d in /usr/local/cargo/registry /usr/local/cargo/git /app/target; do
  if [[ -d "$d" && ! -w "$d" ]]; then
    echo "[devcontainer] fixing ownership of $d"
    sudo chown -R vscode:vscode "$d"
  fi
done

if [[ -f .envrc ]]; then
  if command -v direnv >/dev/null 2>&1; then
    echo "[devcontainer] direnv allow ."
    direnv allow . || echo "[devcontainer] WARN: direnv allow failed (ignored)"
  else
    echo "[devcontainer] direnv not found, skipped"
  fi
fi

if [[ -x .devcontainer/scripts/init-dynamodb.sh ]]; then
  echo "[devcontainer] running init-dynamodb.sh ..."
  bash .devcontainer/scripts/init-dynamodb.sh
else
  echo "[devcontainer] no init-dynamodb.sh found, skipped"
fi

echo "[devcontainer] post-start: done"
