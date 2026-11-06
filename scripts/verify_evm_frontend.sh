#!/usr/bin/env bash
# End-to-end verification of the solc Solidity frontend:
# build, full test suite, cross-contract resolution, and the
# not-a-Foundry-project error path. Exits non-zero on any failure.
set -euo pipefail
cd "$(dirname "$0")/.."

CROSS=tests/fixtures/solc/cross
pass() { printf '  OK   %s\n' "$1"; }
fail() { printf 'FAIL: %s\n' "$1"; exit 1; }
has()  { printf '%s' "$1" | grep -qF "$2"; }

echo "== build =="
cargo build -q -p ilold-cli

echo "== workspace tests =="
cargo test -q --workspace >/dev/null || fail "workspace tests"
pass "all tests green"

echo "== cross-contract resolution ($CROSS) =="
OUT=$(./target/debug/ilold analyze "$CROSS" --contract Vault --verbose)
while IFS= read -r edge; do
  has "$OUT" "$edge" && pass "$edge" || fail "missing call-graph edge: $edge"
done <<'EDGES'
Vault::depositVia → SafeMath::safeAdd
Vault::depositVia → IPool::supply
Vault::depositCast → IPool::supply
EDGES
has "$OUT" "pool::supply" && fail "unresolved placeholder 'pool::supply' present" || pass "no placeholders"

echo "== clear error outside a Foundry project =="
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
echo 'contract X {}' > "$TMP/X.sol"
ERR=$(./target/debug/ilold analyze "$TMP/X.sol" 2>&1 || true)
has "$ERR" "Not a Foundry project" && pass "clear error" || fail "expected 'Not a Foundry project'"

echo "ALL CHECKS PASSED"
