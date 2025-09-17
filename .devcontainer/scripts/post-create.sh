#!/usr/bin/env bash
set -euo pipefail

echo "[devcontainer] Running init.sh ..."

sudo apt-get update
sudo apt-get install -y direnv git

git config --global --add safe.directory /app

rustup component add clippy rustfmt

mkdir -p /home/vscode/.cargo/registry /home/vscode/.cargo/git /app/target
sudo chown -R vscode:vscode /app/target

if ! grep -q 'direnv hook bash' /home/vscode/.bashrc; then
  printf '%s\n' 'if command -v direnv >/dev/null 2>&1; then eval "$(direnv hook bash)"; fi' >> /home/vscode/.bashrc
fi

if [ -f /home/vscode/.zshrc ] && ! grep -q 'direnv hook zsh' /home/vscode/.zshrc; then
  printf '%s\n' 'if command -v direnv >/dev/null 2>&1; then eval "$(direnv hook zsh)"; fi' >> /home/vscode/.zshrc
fi

cargo fetch

echo "[devcontainer] init.sh completed."
