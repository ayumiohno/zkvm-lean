#!/usr/bin/env bash
# Measure gas. Deploys risc0-ethereum's verifier itself and calls claim with a real
# receipt from examples/mathlib/proofs/.
#
#   contracts/gas.sh
#
# risc0-ethereum is cloned on the spot rather than vendored (v3.0.1 pairs with risc0-zkvm 3.0.x).
# Requires Foundry: curl -L https://foundry.paradigm.xyz | bash && foundryup
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
export PATH="$HOME/.foundry/bin:$PATH"
WORK="${LEAN_ZKVM_ETH:-${TMPDIR:-/tmp}/risc0-ethereum}"
TAG=v3.0.1

command -v forge >/dev/null || { echo "forge not found (run foundryup)"; exit 1; }

if [ ! -d "$WORK" ]; then
  git clone --depth 1 --branch "$TAG" https://github.com/risc0/risc0-ethereum "$WORK"
  (cd "$WORK" && git submodule update --init --depth 1 lib/openzeppelin-contracts lib/forge-std)
fi

cp "$ROOT/contracts/TheoremBounty.sol" "$WORK/contracts/src/"
cp "$ROOT/contracts/test/LeanGas.t.sol" "$WORK/contracts/test/"
cd "$WORK/contracts"
forge test --match-contract LeanGas -vv
