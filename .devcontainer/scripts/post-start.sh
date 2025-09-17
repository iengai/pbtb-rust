#!/usr/bin/env bash
set -Eeuo pipefail

echo "[devcontainer] post-start: begin"

cd /app

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
