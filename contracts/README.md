# On-chain verification

Settle a Lean proof on chain in 260 bytes, without revealing the proof term.

```
proof term 3 MB
  → kernel type-check in the zkVM  → succinct receipt 224,210 B   (GPU, 6.8 s)
  → wrap in Groth16                → seal 256 B                   (CPU, ~36 s)
  → prepend the selector           → 260 B of calldata
```

| | |
| --- | --- |
| `TheoremBounty.sol` | a bounty paid to the first prover of a proposition (40 lines) |
| `gas.sh` · `test/LeanGas.t.sol` | gas measurement with Foundry |
| `compress` (`zkvm/host/src/bin/`) | succinct → Groth16 |
| `calldata` (`zkvm/host/src/bin/`) | receipt → the three arguments `verify()` takes |

## Values for `Practical.thm_infinitude`

```sh
calldata ../examples/mathlib/proofs/receipt_infinitude_g16.bin
```
```
imageId       = 0x44088753d3612b3b5641daf6a7af9f962211854afe91693183d3719d1e1350c7
journalDigest = 0x31f44e449e0d6d44aac6c8502505335243a6e6ea85c31a84e61909607979808b
seal          = 0x73c457ba25aeef60...c2c1d4e4c   (260 bytes)
```

That one `journalDigest` commits to the prelude SHA-256, the record and name Merkle
roots, the theorem name, the canonical statement digest and the skipped-axiom set, so
the 492-byte journal never goes on chain.

Groth16 is randomised: re-wrapping the same receipt yields a different seal, equally
valid, with `imageId` and `journalDigest` unchanged.

```sh
cast send --value 1ether --create $(solc --bin TheoremBounty.sol) \
     "constructor(address,bytes32,bytes32)" $VERIFIER $IMAGE_ID $JOURNAL_DIGEST
cast send $BOUNTY "claim(bytes)" $SEAL      # the proof term is not passed
```

## Measured gas

`gas.sh` clones risc0-ethereum v3.0.1, deploys `RiscZeroGroth16Verifier` itself, and
calls `claim` with a real seal. No external RPC is needed — Foundry is the only
prerequisite.

```sh
./gas.sh
```
```
verifier.verify      231,628 gas    pairing check
bounty.claim         288,720 gas    verify + SSTORE + transfer + event
calldata 356 bytes     4,616 gas
intrinsic + data      25,616 gas
one transaction      314,336 gas    ≈ $28 at 30 gwei; ~100× less on L2
```

Deployed bytecode is 1,427 bytes (`solc 0.8.26`, `via_ir`, `optimizer_runs = 10000`).

This also confirms version compatibility: a seal from risc0-zkvm 3.0.6 verifies under
the risc0-ethereum 3.0.1 verifier, so selector and control IDs agree. A mismatch would
revert.

## Known limitations

1. **One deployment per proposition.** The journal contains variable-length
   `String`/`Vec<String>`, which is awkward to parse in Solidity, so it is folded into
   one digest fixed at deploy time. A general registry would have to parse it.
2. **Front-running.** The seal is public calldata, so a third party can claim first.
   Binding the claimant's address into the journal would fix it, but the journal would
   then vary per claimant and could no longer be fixed at deploy time.
3. **The trust assumption is unchanged.** `journalDigest` pins *which* environment was
   assumed, but not that it type-checks (`assumed` mode).
4. **Groth16 generation fails on Turing GPUs** (sppark's BN254 MSM hits an illegal
   memory access on sm_75), so the STARK runs on GPU and the SNARK wrap on CPU. The
   first wrap also spends 206 s loading the proving key.

Full context in [docs/results.md](../docs/results.md).
