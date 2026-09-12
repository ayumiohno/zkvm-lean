#!/usr/bin/env bash
# Booth demo.
#
#   scripts/demo.sh          run straight through
#   scripts/demo.sh -s       pause for a keypress at each step, for narrating
#
# Proving takes 71 s on GPU (about an hour on CPU), so a pre-generated receipt in
# demo/ is used. What this shows is the **verifier's side**, which takes 0.1 s.
set -uo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
D="$ROOT/demo"
V="$ROOT/zkvm/target/release/verify"
STEP=false
[ "${1:-}" = "-s" ] && STEP=true

# Generate the receipt ahead of time; proving is slow (GPU recommended, see demo/README.md).
if [ ! -f "$D/receipt.bin" ]; then
    cat >&2 <<'MSG'
demo/receipt.bin is missing. Generate it first (71 s on GPU):

  compose demo/public.bin demo/full.bin Preimage.knows_preimage           --assume-prelude --prove --out demo/receipt.bin

MSG
    exit 1
fi
# Rebuild the tampered copy each run, so it tracks a regenerated receipt.
python3 - "$D/receipt.bin" "$D/tampered.bin" <<'PYEOF'
import sys
d = bytearray(open(sys.argv[1], 'rb').read())
d[len(d) // 2] ^= 0xff
open(sys.argv[2], 'wb').write(d)
PYEOF

b() { printf '\033[1m%s\033[0m\n' "$*"; }
dim() { printf '\033[2m%s\033[0m\n' "$*"; }
pause() { $STEP && { printf '\033[2m  [Enter]\033[0m'; read -r _; } || sleep 1; }
rule() { printf '\033[2m%s\033[0m\n' "────────────────────────────────────────────────────────"; }

clear
b "Verifying a Lean 4 proof without revealing its contents"
dim "zk-tokyo/advanced-cryptography-2026#23"
echo
rule

# ── 1 ──────────────────────────────────────────────────────
echo
b "1. What we want to prove"
echo
cat <<'LEAN'
    def f (x : Nat) : Nat := (x * x + 7) % 1000003        public

    theorem knows_preimage : ∃ x, x < 100000 ∧ f x = 4630 :=
      ⟨31337, by decide, by decide⟩
    --  ^^^^^ this is what we hide
LEAN
echo
dim "  Show knowledge of an x with f x = 4630, without revealing x."
pause

# ── 2 ──────────────────────────────────────────────────────
echo; rule; echo
b "2. Is the secret really only on the private side?"
echo
printf "    %-22s %s\n" "public part"      "$(grep -c '\"natVal\":\"31337\"' "$D/public.ndjson" 2>/dev/null | head -1) occurrences"
printf "    %-22s %s\n" "with the proof term" "$(grep -c '\"natVal\":\"31337\"' "$D/full.ndjson" 2>/dev/null | head -1) occurrences"
echo
dim "  The verifier receives only the receipt, never the proof term."
pause

# ── 3 ──────────────────────────────────────────────────────
echo; rule; echo
b "3. What the verifier receives"
echo
printf "    receipt   %8s bytes\n" "$(wc -c <"$D/receipt.bin" | tr -d ' ')"
printf "    of which public (journal) %4s bytes\n" "$("$ROOT/zkvm/target/release/inspect" "$D/receipt.bin" 2>/dev/null | sed -n 's/.*journal (public) *: \([0-9]*\).*/\1/p')"
echo
dim "  The proof term (54,390 bytes) is nowhere in it."
pause

# ── 4 ──────────────────────────────────────────────────────
echo; rule; echo
b "4. Verify"
dim "  The verifier holds only the receipt, their public environment, and their own proposition."
echo
t0=$(python3 -c 'import time;print(time.time())')
"$V" "$D/receipt.bin" "$D/public.bin" "$D/expected.ndjson" Preimage.knows_preimage \
     --expected-name Verifier.expected --manifest "$D/manifest.json" 2>&1 | sed 's/^/    /'
t1=$(python3 -c 'import time;print(time.time())')
echo
b "    elapsed: $(python3 -c "print(f'{$t1-$t0:.2f} s')")"
dim "    (generating the proof takes 71 s on GPU, about an hour on CPU)"
pause

# ── 5 ──────────────────────────────────────────────────────
echo; rule; echo
b "5. Lies do not get through"
echo
dim "  a) if the verifier expected a different proposition"
"$V" "$D/receipt.bin" "$D/public.bin" "$D/other.ndjson" Preimage.knows_preimage \
     --expected-name Verifier.expected --manifest "$D/manifest.json" 2>&1 | tail -2 | sed 's/^/    /'
echo
dim "  b) if the verifier's public environment differs"
"$V" "$D/receipt.bin" "$D/other_public.bin" "$D/expected.ndjson" Preimage.knows_preimage \
     --expected-name Verifier.expected --manifest "$D/manifest.json" 2>&1 | tail -3 | sed 's/^/    /'
echo
dim "  c) if one byte of the receipt is altered"
"$V" "$D/tampered.bin" "$D/public.bin" "$D/expected.ndjson" Preimage.knows_preimage \
     --expected-name Verifier.expected --manifest "$D/manifest.json" 2>&1 | tail -1 | sed 's/^/    /'
echo
rule
echo
b "  Confirmed the intended proposition was proved, without ever seeing the proof term."
echo
