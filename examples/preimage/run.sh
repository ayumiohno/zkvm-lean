#!/usr/bin/env bash
# Demo: verify a proof of preimage knowledge in the zkVM without revealing the secret.
#
#   1. build the theorem in Lean
#   2. export the statement side alone, and the whole theorem, to NDJSON
#   3. confirm the secret 31337 appears only on the proof-term side
#   4. type-check in the zkVM and count cycles
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
EXP="$ROOT/vendor/lean4export/.lake/build/bin/lean4export"
HOST="$ROOT/zkvm/target/release/host"
OUT="$ROOT/out/preimage"
export PATH="$HOME/.risc0/bin:$HOME/.cargo/bin:$PATH"
mkdir -p "$OUT"
cd "$HERE"

echo "==> 1. build with Lean"
lake build

echo
echo "==> 2. export (public = f's dependency closure / full = the whole theorem)"
lake env "$EXP" Preimage -- Preimage.f              > "$OUT/public.ndjson"
lake env "$EXP" Preimage -- Preimage.knows_preimage > "$OUT/full.ndjson"
printf "    public.ndjson  %8s bytes\n" "$(wc -c <"$OUT/public.ndjson" | tr -d ' ')"
printf "    full.ndjson    %8s bytes\n" "$(wc -c <"$OUT/full.ndjson"   | tr -d ' ')"

echo
echo "==> 3. which numbers appear on which side"
printf "    %-10s %-12s %-8s %s\n" natVal "public(f)" full origin
for v in 1000003 100000 4630 31337; do
  a=$(grep -c "\"natVal\":\"$v\"" "$OUT/public.ndjson") || a=0
  b=$(grep -c "\"natVal\":\"$v\"" "$OUT/full.ndjson")   || b=0
  case $v in
    1000003) d="f's definition (public)";;
    100000)  d="the statement's bound (public)";;
    4630)    d="the statement's target (public)";;
    31337)   d="* the secret, inside the proof term";;
  esac
  printf "    %-10s %-12s %-8s %s\n" "$v" "$a" "$b" "$d"
done
echo
echo "    -> 31337 appears only on the proof-term side. That is what gets hidden."
grep -n '"natVal":"31337"' "$OUT/full.ndjson" | sed 's/^/    /'

echo
echo "==> 4. type-check in the zkVM (executor only; no proof generated)"
printf "    %-28s %8s %6s %10s %10s %12s\n" theorem bytes decl parse check TOTAL
for t in knows_preimage knows_preimage_iter10 knows_preimage_iter100 knows_preimage_iter1000; do
  f="$OUT/$t.ndjson"
  [ -f "$f" ] || lake env "$EXP" Preimage -- "Preimage.$t" > "$f"
  o=$(RUST_LOG=warn "$HOST" "$f" 2>&1)
  p=$(sed -n 's/.*parse=\([0-9]*\).*/\1/p' <<<"$o"); c=$(sed -n 's/.*check=\([0-9]*\).*/\1/p' <<<"$o")
  d=$(sed -n 's/.*declars=\([0-9]*\).*/\1/p' <<<"$o"); T=$(sed -n 's/.*EXECUTED cycles=\([0-9]*\).*/\1/p' <<<"$o")
  printf "    %-28s %8s %6s %10s %10s %12s\n" "$t" "$(wc -c <"$f"|tr -d ' ')" "$d" "$p" "$c" "$T"
done
