# Results

Measured facts about this implementation. Every number here was produced by this
repository; nothing is quoted from literature.

See also: [design](design.md) · [soundness](soundness.md) · [implementation](implementation.md)

| | |
| --- | --- |
| Prover (GPU) | Quadro RTX 8000, Linux/x86_64 |
| Prover (CPU) / verifier | M4 Mac, macOS/arm64 |
| Stack | RISC Zero 3.0.6, risc0-ethereum 3.0.1, Lean 4.34.0-rc2 |
| Guest image ID | `44088753d3612b3b5641daf6a7af9f962211854afe91693183d3719d1e1350c7` |

## Status

| | |
| --- | --- |
| Lean → export → kernel type-check in the zkVM → proof → verification | ✅ |
| Public / private split (the proof term stays hidden) | ✅ |
| Independent verifier (6 checks, with tamper-detection negative tests, see [soundness](soundness.md)) | ✅ |
| Axiom audit (with negative tests) | ✅ |
| Merkle-authenticated sparse witness | ✅ |
| Trust modes `certified` / `assumed` | ✅ |
| Native prelude check for verifiers (`checkprelude`) | ✅ |
| Reproducible Docker builds (identical image ID across arm64 / amd64) | ✅ |
| Verification without building the guest (manifest) | ✅ |
| Composition (Stage 1 receipt consumed by Stage 2) | ✅ |
| Mathlib-scale proofs on GPU | ✅ |
| Groth16 compression and on-chain verification | ✅ |

## Proven theorems

Five theorems from `examples/mathlib/Practical.lean`, all against **the same public
prelude** and all with `--assume-prelude`. Receipts are in
`examples/mathlib/proofs/`; the proof terms are not.

| Theorem | Cycles | Padded | GPU proof | Receipt |
| --- | ---: | ---: | ---: | --- |
| `thm_infinitude` — there are infinitely many primes | 1.76M | 2,359,296 | **6.8 s** | ✅ |
| `thm_sqrt2` — √2 is irrational | 2.62M | 3,407,872 | 10.1 s | ✅ |
| `thm_gauss` — Gauss sum | 3.66M | 4,718,592 | 12.9 s | ✅ |
| `thm_factor` — knowing a factorisation of 3233 | 30.55M | 37,748,736 | 95.2 s | ✅ |
| `thm_amgm` — AM–GM over ℝ | 61.96M | 77,725,696 | 197.2 s | ✅ |

*Cycles* is the executed cycle count; *padded* is the segment length the prover
actually proves, which is what the wall-clock time tracks.

Every receipt reports `declarations checked: 1`. Verification takes **0.06–0.07 s**
and does not need a Lean installation. All eleven receipts in the directory were
re-verified cryptographically on macOS/arm64 — **11 / 11 pass** — which also
confirms that the Docker build chain produces the same guest on both platforms.

`thm_factor`'s statement is `∃ p q, p.Prime ∧ q.Prime ∧ p ≠ q ∧ p * q = 3233`.
**61 and 53 appear nowhere in the receipt**; they exist only in the proof term.

The shared public prelude:

```
size          682,879,924 bytes
declarations       214,101
records         37,277,329
SHA-256       7d539df7b3e795c7f7bd7ea1327687cd3f5083fdd958419ce8a14028653e89a1
Merkle root   1194e86945579ca116c57c917f25c1b8ffd523998732049cf9d974ed3c36a963
```

It is too large to track in git, so `examples/mathlib/proofs/roots.txt` records how
to rebuild it. Export is deterministic: the same inputs reproduce it byte for byte.

## Cost is independent of environment size

This is the central result. A proof's cost is governed by what the proof *touches*,
not by how large the ambient environment is.

The number of touched declarations does not grow with the environment — growing the
environment 2.2× took the touched set from 1,680 down to 671. A Merkle-authenticated
sparse witness delivers only those records to the guest.

Measured across a **168× environment**:

| | Small | Large | Ratio |
| --- | ---: | ---: | ---: |
| Public prelude | 1,265 declarations | 213,144 declarations | **168x** |
| Selected leaves | 11,635 | 11,999 | 1.03x |
| Type-check | 29.16M | 29.11M | 1.00x |
| **Stage 2 total** | **62.97M** | **67.71M** | **1.08x** |

Two implementation defects contradicted this claim before it held: a map indexed by
pre-compaction indices, and a set of public declaration names passed in whole. Both
scaled with the environment. Fixing them took Stage 2 from 704.3M to 67.7M cycles
(**10.4×**). The claim was only true after it was measured.

A consequence: making the entire `ring` machinery public costs nothing
(`declarations checked: 1`). Fixing Mathlib as a public prelude is sound as an
engineering choice, not just a conceptual one.

## On-chain verification

A succinct receipt is 224 KB and cannot be used on the EVM. Wrapping it in Groth16
produces a constant-size SNARK.

| | Succinct | Groth16 |
| --- | ---: | ---: |
| Receipt file | 224,210 B | **1,481 B** |
| Seal | 223,718 B | **256 B** |
| Journal | 492 B | 492 B (unchanged) |
| Wrapping time | — | 35.6 s (CPU; 206 s on the first run, loading the proving key) |

Gas, measured against `RiscZeroGroth16Verifier` from risc0-ethereum 3.0.1 on a local
chain (`contracts/gas.sh` clones, deploys and runs it; no external RPC):

```
verifier.verify      231,628 gas    pairing check
bounty.claim         288,720 gas    verify + SSTORE + transfer + event
calldata 356 bytes     4,616 gas
intrinsic + data      25,616 gas
one transaction      314,336 gas    ≈ $28 at 30 gwei; ~2 orders of magnitude less on L2
```

`contracts/TheoremBounty.sol` fixes `imageId` and `journalDigest` at deploy time. The
journal never goes on chain: a single 32-byte digest commits to the prelude SHA-256,
the record root, the declaration-name root, the theorem name, the statement digest
and the skipped-axiom set. **A 3 MB proof term is settled by 260 bytes of calldata.**

This also confirms version compatibility: a seal produced by risc0-zkvm 3.0.6
verifies under the risc0-ethereum 3.0.1 verifier, so the selector and control IDs
agree. A mismatch would revert.

Known gaps in the contract:

- **One deployment per proposition**, because the journal digest is immutable.
  A `mapping(bytes32 => uint256)` from journal digest to bounty would lift this.
- **Front-running.** The seal is public calldata, so a third party can claim first.
  Binding the claimant's address into the journal would fix it, but then the journal
  varies per claimant and cannot be fixed at deploy time.
- The `assumed` trust mode carries over unchanged; putting a proof on chain does not
  make the prelude self-certifying.
- Groth16 wrapping fails on Turing GPUs, so the SNARK step runs on CPU.

## Zero-knowledge holds only for some receipt types

ZK for a STARK comes from randomised padding at the tail of the trace, so that FRI
openings reveal noise instead of execution values. RISC Zero says as much
(`risc0-zkp/src/prove/poly_group.rs`: *"…is zero knowledge so long as there is
sufficient randomized padding"*), and leaves the padding to the caller.

`ZK_CYCLES` noise is injected in `risc0-circuit-recursion` only. The
`risc0-circuit-rv32im` circuit, which proves segments, injects none.

| Receipt | Contents | ZK |
| --- | --- | --- |
| Composite | a list of segment STARKs (rv32im) | ❌ no padding |
| Succinct | one recursion STARK (segment seals are consumed) | ⭕️ noise injected |
| Groth16 | the succinct receipt wrapped by rapidsnark | ⭕️ randomised, see below |

A composite receipt additionally exposes its cycle count structurally — it is a
`Vec<SegmentReceipt>` whose seal sizes reveal each segment's po2. In this project
cycle count correlates with proof-term size, so that is a real leak. **All receipts
published here are succinct or Groth16.**

Groth16 randomisation was verified empirically. Wrapping the same succinct receipt
twice yields files differing in exactly 256 contiguous bytes — the proof body — while
the selector, `imageId`, `journalDigest` and journal are bit-identical. Of the seal's
512 hex digits, 480 differ, matching the 15/16 expected of uniform randomness.
`receipt_infinitude_g16_alt.bin` is the second wrapping; both verify.

Channels outside the proof system remain. RISC Zero closes one explicitly, seeding
128 bits of entropy into the guest's memory image so that the post-state digest does
not leak it. **Proving time still leaks**: 6.8 s versus 197.2 s distinguishes the
size of the hidden proof term.

## Performance

All figures are executor-measured on `Preimage.knows_preimage` unless stated otherwise.

### 175.4M → 17.6M cycles

| Configuration | verify | parse | check | other | **total** |
| --- | ---: | ---: | ---: | ---: | ---: |
| (a) one stage, serde input | — | 44.8M | 73.8M | 56.9M | **175.4M** |
| (b) one stage, raw bytes | — | 44.8M | 73.7M | 0.7M | **119.2M** |
| (c) two stages, raw bytes | 0.36M | 45.1M | **9.6M** | 0.4M | **55.4M** |
| (d) two stages, binary format | 0.10M | **7.4M** | 9.6M | 0.4M | **17.6M** |

Three changes account for it.

1. **Composition** took check from 73.8M to 9.6M by not re-checking a 204-declaration
   prelude. `env::verify` itself costs 0.36M, so composition is nearly free.
2. **Input encoding.** RISC Zero's serde encodes `Vec<u8>` as one u32 word per byte —
   about **150 cycles per byte**, or 56M cycles just to hand over 376 KB. Passing a
   length plus raw words removed it.
3. **Dropping NDJSON.** Instrumenting nanoda showed 90% of parse (40.5M of 45.1M) was
   typed JSON deserialisation — which was *slower* than generic `serde_json::Value`
   parsing (33.3M), because `#[serde(flatten)]` buffers each object into an
   intermediate representation and deserialises it again. A record type without
   flatten, encoded with postcard, cut parse to 7.4M and the file from 376 KB to 97 KB.

The first probe measured the wrong path (`serde_json::Value`, not the typed
deserialisation nanoda actually uses); the flatten effect only became visible after
instrumenting nanoda itself.

### What the cost is proportional to

| | Proportional to |
| --- | --- |
| **parse** | the size of the environment |
| **check** | how much computation the proof forces on the kernel |

Applying `f` once costs about 269K cycles: ~108K for arithmetic and ~161K for
unfolding the recursor (`brecOn`). Most of the arithmetic is not arithmetic but delta
reduction down the type-class stack — `x * x → HMul.hMul → Mul.mul → instMulNat →
Nat.mul` — before GMP is finally reached.

### How the proof is written matters

An optimisation axis entirely separate from the zkVM (`examples/preimage/Style.lean`):

| Proposition | Written as | parse | check | Declarations |
| --- | --- | ---: | ---: | ---: |
| `big = 80779853383` | `by decide` | 2.69M | **11.70M** | 94 |
| `big = 80779853383` | `rfl` | 1.04M | **0.80M** | 52 |
| `n < 100 → n < 200` | with `by decide` | 5.10M | 37.37M | 151 |
| `n < 100 → n < 200` | composing lemmas | 8.35M | **79.47M** | 243 |

**`rfl` is 14.6× cheaper than `by decide`** for the same proposition. `by decide`
builds `of_decide_eq_true (Eq.refl true)`, forcing the kernel through `Decidable` and
`Bool`; `rfl` just asks whether both sides are definitionally equal.

The expectation that structural proofs would be cheaper was wrong — composing lemmas
cost more than twice `by decide`, because the dependency closure grew from 151 to 243
declarations. What matters is (1) how much the kernel is made to compute and (2) how
much prelude gets pulled in.

**(2) only applies when the public prelude is small.** With Mathlib public, lemmas live
in the prelude and cost nothing to cite, leaving only (1) — so applying an existing
lemma becomes the cheapest possible proof, and this conclusion inverts.

### Merkle verification

Once sparse witnesses landed, authentication was the only fixed cost left. SHA floors
measured inside the zkVM: `Impl::compress` 165 cycles, `hash_bytes` on 53 bytes 398,
`Digest::try_from` 75. Verification makes 1,343 leaf plus 1,973 node calls, and the
same traversal without hashing costs only 314K — so essentially all of it was the
hash path. Yet measured costs were far above the bare calls: `leaf_hash` 892 against
398, `node_hash` 586 against 165.

| Step | merkle | Stage 2 total |
| --- | ---: | ---: |
| Initial | 4.64M | 10.71M |
| Internal nodes via `Impl::compress` | 3.89M | 9.96M |
| Removing the per-node binary search | 2.93M | 9.00M |
| **Fixing the hash path** | **1.73M** | **7.80M** |

62% below the 20.48M of full mode. The wins were:

- **`digest(&[&[u8]])` overhead.** Totalling lengths and copying four slices cost more
  than SHA itself (+494 cycles per leaf). Leaf headers are fixed-width, so they are now
  built directly.
- **`[u8; 32]` ↔ `Digest` conversions**, 238 cycles per internal node. `Digest` is now
  carried through the traversal, converting only when reading proof nodes and at the
  final comparison.
- **A per-node binary search.** `rebuild` ran `partition_point` twice over the entire
  1,343-element selection at every node — a cost that grows with the tree, inside the
  mechanism whose whole purpose is to be independent of environment size. Ranges are
  now passed down and only the subrange is searched.
- Internal nodes use `Impl::compress` directly. Prefixing a domain tag makes the input
  65 bytes, which SHA padding expands to two blocks; domain separation instead comes
  from the initial state `SHA-256("lean-zkvm/merkle/internal/v1")`.

Three diagnoses were wrong before this one (heap allocation, recursion overhead,
iteration), each disproved by a microbenchmark.

### Throughput

| CPU (M4 Mac) | cycles | time | cycles/s |
| --- | ---: | ---: | ---: |
| Stage 1 (succinct) | 3,145,728 | 491.9 s | 6,395 |
| Stage 2 (succinct) | 2,097,152 | 355.4 s | 5,901 |
| One stage (composite) | 4,194,304 | 583.8 s | 7,184 |

| GPU (Quadro RTX 8000, succinct) | cycles | time | cycles/s |
| --- | ---: | ---: | ---: |
| Preimage Stage 1 | 137,887,744 | 372.8 s | 370K |
| Preimage Stage 2 | 26,214,400 | 69.6 s | 377K |
| Mathlib `ring` Stage 2 | 84,934,656 | 213.1 s | 399K |
| Mathlib `ring` Stage 2 (statement-only prelude) | 1,088,421,888 | 2,961.7 s | 367K |

**About 370K cycles/s, 63× the M4 CPU.** Throughput is linear in work: four
configurations spanning an order of magnitude all land between 367K and 399K. Succinct
proving is only 15–20% slower than composite, so composition is cheap.

**Cycles translate almost directly into proving time**, which is what justifies
optimising against the executor rather than against the prover.

### Other findings

- The choice of public prelude changes cost by up to **3,522×**
  (`thm_infinitude`: 6,186M with a statement-only prelude, 1.76M with Mathlib).

## Limitations

**The TCB does not shrink; it moves.** Trusting Lean is replaced by trusting
`nanoda_lib` and RISC Zero.

**Stage 1 is not needed.** Proving that the prelude type-checks inside the zkVM costs
about 540 billion cycles for Mathlib — roughly 17 GPU-days. The same check runs
natively in **0.08 s** (`checkprelude`, 1,265 declarations), and a verifier needs no
Lean installation to run it. The composition mechanism remains useful — Stage 2 is
structured to skip the prelude — but its original motivation was wrong.

**The sparse witness is still required.** It is what keeps the per-proof cost off the
environment size, and no native check can substitute for it.

**This is not faster than `lake build`** (25 ms versus minutes), and it is not meant
to be. ZK pays off when verifiers are numerous, cannot trust the prover, or are not
human.

**The verifier still has to read the statement**, as a raw kernel-level `Expr`. "No
Lean installation required" is achievable; "no Lean knowledge required" is not.

**Tactic scripts cannot be hidden, because they no longer exist at export time.**
They have already been elaborated into a proof term. What can be hidden is the
lemma structure.

## Not yet done

- **Full Mathlib** (497,421 constants). 213,144 declarations (43%) is confirmed; the
  remainder should only add two levels to the Merkle tree.
- **Certifying Mathlib with Stage 1.** ~540 billion cycles; reusable once produced,
  but the current guest cannot hold the whole environment in memory.
- **Caching the host-side prelude parse.** Now that the zkVM side is ~7 s, natively
  parsing the 683 MB prelude dominates wall-clock time (about 4 minutes).
- Record the Mathlib revision and the lean4export commit in the manifest.
- Replace postcard decoding with a hand-written fixed-width decoder (constant factor).
- `--full` with 683 MB makes the guest's SHA-256 disagree with the host's; sparse is
  the default, so this is not on any live path.
- Content-addressing terms (git-style) would remove sibling hashes, reducing the
  Merkle tree to a small name-to-digest table.
