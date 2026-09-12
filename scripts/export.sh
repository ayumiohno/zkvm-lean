#!/usr/bin/env bash
# Export only the named theorem to NDJSON, including its transitive dependencies.
# usage: scripts/export.sh <ConstName> [outfile]
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
EXPORTER="$ROOT/vendor/lean4export/.lake/build/bin/lean4export"
NAME="$1"
OUT="${2:-$ROOT/out/$(echo "$NAME" | tr '.' '_').ndjson}"
mkdir -p "$(dirname "$OUT")"
cd "$ROOT/lean"
lake env "$EXPORTER" ZkDemo -- "$NAME" > "$OUT"
echo "$OUT ($(wc -c < "$OUT" | tr -d ' ') bytes, $(wc -l < "$OUT" | tr -d ' ') lines)"
