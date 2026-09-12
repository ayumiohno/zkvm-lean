#!/usr/bin/env bash
# Fetch the external exporter and build it together with the in-repo nanoda fork.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
mkdir -p "$ROOT/vendor"

if [ ! -d "$ROOT/vendor/lean4export" ]; then
  git clone --depth 1 https://github.com/leanprover/lean4export.git "$ROOT/vendor/lean4export"
fi
(cd "$ROOT/vendor/lean4export" && lake build)
(cd "$ROOT/lean" && lake build)
(cd "$ROOT/forks/nanoda_lib" && cargo build --release)   # the tracked research fork
(cd "$ROOT/zkvm" && cargo build --release)
echo "setup complete"
