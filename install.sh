#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")"

missing=()
for tool in cargo node npm; do
  command -v "$tool" >/dev/null 2>&1 || missing+=("$tool")
done
if ((${#missing[@]})); then
  echo "ilold needs: ${missing[*]}" >&2
  echo "  Rust (cargo): https://rustup.rs" >&2
  echo "  Node.js (node, npm): https://nodejs.org" >&2
  exit 1
fi

echo "==> Building the web app"
npm --prefix crates/ilold-web/frontend ci
npm --prefix crates/ilold-web/frontend run build

echo "==> Installing ilold"
cargo install --path crates/ilold-cli --locked --force "$@"

echo
echo "Next:   ilold explore path/to/foundry-project"
echo "Update: git pull && ./install.sh"
