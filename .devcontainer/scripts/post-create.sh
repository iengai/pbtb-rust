#!/usr/bin/env bash
set -euo pipefail

echo "[devcontainer] Running init.sh ..."

sudo apt-get update
sudo apt-get install -y direnv git

git config --global --add safe.directory /app

rustup component add clippy rustfmt

mkdir -p /usr/local/cargo/registry /usr/local/cargo/git /app/target
# These are freshly-created named volumes (root-owned by default); the cargo cache
# dirs and the target dir must be writable by the vscode user or cargo fails with
# "Permission denied" when populating the registry. CARGO_HOME is /usr/local/cargo
# (matching the rust feature), so the cache volumes are mounted there.
sudo chown -R vscode:vscode /app/target /usr/local/cargo/registry /usr/local/cargo/git

if ! grep -q 'direnv hook bash' /home/vscode/.bashrc; then
  printf '%s\n' 'if command -v direnv >/dev/null 2>&1; then eval "$(direnv hook bash)"; fi' >> /home/vscode/.bashrc
fi

if [ -f /home/vscode/.zshrc ] && ! grep -q 'direnv hook zsh' /home/vscode/.zshrc; then
  printf '%s\n' 'if command -v direnv >/dev/null 2>&1; then eval "$(direnv hook zsh)"; fi' >> /home/vscode/.zshrc
fi

cargo fetch

echo "[devcontainer] init.sh completed."
