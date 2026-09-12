#!/usr/bin/env bash
# Measure zkVM cycles per theorem (executor only; no proof generation).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
export PATH="$HOME/.risc0/bin:$HOME/.cargo/bin:$PATH"
HOST="$ROOT/zkvm/target/release/host"
THMS="${*:-mynat_add_zero mynat_add_succ mynat_zero_add nat_lit_add nat_lit_big}"
printf "%-18s %8s %6s %10s %10s %10s %12s\n" theorem bytes decl parse check other TOTAL
for t in $THMS; do
  f="$ROOT/out/$t.ndjson"
  [ -f "$f" ] || "$ROOT/scripts/export.sh" "ZkDemo.$t" "$f" >/dev/null
  o=$(RUST_LOG=warn "$HOST" "$f" 2>&1)
  p=$(sed -n 's/.*parse=\([0-9]*\).*/\1/p' <<<"$o"); c=$(sed -n 's/.*check=\([0-9]*\).*/\1/p' <<<"$o")
  d=$(sed -n 's/.*declars=\([0-9]*\).*/\1/p' <<<"$o"); T=$(sed -n 's/.*EXECUTED cycles=\([0-9]*\).*/\1/p' <<<"$o")
  printf "%-18s %8s %6s %10s %10s %10s %12s\n" "$t" "$(wc -c <"$f"|tr -d ' ')" "$d" "$p" "$c" "$((T-p-c))" "$T"
done
