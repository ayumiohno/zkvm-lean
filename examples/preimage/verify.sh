#!/usr/bin/env bash
# A demo of the public/private split.
#
#   prover  : build a zk proof of having proved the proposition, proof term hidden
#   verifier: compare the digest of their own proposition against the journal
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
EXP="$ROOT/vendor/lean4export/.lake/build/bin/lean4export"
OUT="$ROOT/out/preimage"
export PATH="$HOME/.risc0/bin:$HOME/.cargo/bin:$PATH"
mkdir -p "$OUT"
cd "$HERE"
lake build >/dev/null

echo "==> 1. prover: export the public part, and the whole thing including the proof"
#   constants are emitted in the order given after --, so public is a byte prefix of full
lake env "$EXP" Preimage -- Preimage.f                          > "$OUT/public.ndjson"
lake env "$EXP" Preimage -- Preimage.f Preimage.knows_preimage  > "$OUT/full.ndjson"
pub=$(wc -c <"$OUT/public.ndjson" | tr -d ' ')
full=$(wc -c <"$OUT/full.ndjson" | tr -d ' ')
echo "    public : $pub bytes  (also given to the verifier)"
echo "    private: $((full - pub)) bytes  (the proof term; never handed over)"
echo "    the secret 31337 is only on the private side: $(grep -c '"natVal":"31337"' "$OUT/public.ndjson" | head -1) / $(grep -c '"natVal":"31337"' "$OUT/full.ndjson" | head -1)"

echo
echo "==> 2. verifier: write the proposition yourself (proof is sorry)"
lake env "$EXP" Verifier -- Verifier.expected > "$OUT/expected.ndjson"
echo "    occurrences of the secret 31337: $(grep -c '"natVal":"31337"' "$OUT/expected.ndjson" | head -1)"
want=$("$ROOT/zkvm/target/release/stmtdigest" "$OUT/expected.ndjson" Verifier.expected | cut -d' ' -f1)
echo "    the expected statement digest:"
echo "      $want"

echo
echo "==> 3. convert NDJSON to the binary format (6x cheaper to parse in the zkVM)"
"$ROOT/zkvm/target/release/tobin" "$OUT/public.ndjson" "$OUT/public.bin" | sed 's/^/    /'
"$ROOT/zkvm/target/release/tobin" "$OUT/full.ndjson"   "$OUT/full.bin"   | sed 's/^/    /'

echo
echo "==> 4. prover: prove in the zkVM"
echo "    executor only here. For a real proof add --prove --out <receipt.bin>:"
echo "      compose $OUT/public.bin $OUT/full.bin Preimage.knows_preimage --prove --out $OUT/receipt.bin"
"$ROOT/zkvm/target/release/compose" "$OUT/public.bin" "$OUT/full.bin" Preimage.knows_preimage \
  2>/dev/null | sed 's/^/    /'

echo
echo "==> 5. verifier: verify the receipt independently"
if [ -f "$OUT/receipt.bin" ]; then
  #   the receipt is the only thing received from the prover.
  #   the verifier supplies public.bin and the proposition they wrote.
  "$ROOT/zkvm/target/release/verify" \
      "$OUT/receipt.bin" "$OUT/public.bin" "$OUT/expected.ndjson" Preimage.knows_preimage \
      --expected-name Verifier.expected \
      | sed 's/^/    /'
else
  echo "    (skipped: no receipt. Generate one with --prove --out and rerun to verify.)"
  echo
  echo "    verify checks these nine. The statement digest alone is not enough:"
  echo "    without the prelude digest, substituted definitions go undetected."
  echo "      1. the receipt is valid for THMCHECK_ID"
  echo "      2. the journal env image ID is the official ENVCHECK_ID"
  echo "      3. the prelude digest matches the verifier's public.bin"
  echo "      4. the theorem name is as expected"
  echo "      5. the statement digest matches the verifier's named proposition"
  echo "      6. no axioms outside the allowlist were used"
  echo "      7. the private suffix cannot have introduced an allowlisted axiom"
  echo "      8. every constant in the statement is defined by the public environment"
  echo "      9. the verifier's file and the public environment agree on what those mean"
fi
