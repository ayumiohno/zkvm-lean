# lean-zkvm

**Prove that a Lean 4 theorem type-checks — without revealing the proof term.**

The verifier needs no Lean installation and never runs `lake build`; they check a zk
proof. Built for [zk-tokyo/advanced-cryptography-2026#23](https://github.com/zk-tokyo/advanced-cryptography-2026/issues/23),
with performance as the primary axis: where the cost lands, and how far it scales.

```
Euclid's theorem — there are infinitely many primes

  proof term      3 MB          never reaches the verifier
  proving         6.8 s         one GPU
  verification    0.07 s        a laptop, no Lean
  on chain        314,336 gas   a 260-byte seal settles it
```

## How it works

Putting all of `lake build` in a zkVM is not viable — the elaborator and tactic
framework are enormous. But Lean's trusted computing base is **the kernel alone**:
whatever tactics produced a proof term, the theorem holds if the kernel accepts it
(the de Bruijn criterion). So only type-checking goes in the zkVM.

```
[host — untrusted]              [zkVM guest — RISC Zero]        [verifier]
lake build                      nanoda_lib type-checks           checks a
  → lean4export        →        with Lean's kernel rules    →    receipt
     (NDJSON export)            failure ⇒ panic ⇒ no proof       (0.07 s)
```

The statement is public; the proof term stays in the witness. The journal carries a
canonical digest of the theorem's type, the prelude commitment and any axioms used, so
a verifier learns **what** was proved without learning **why** it is true.

| | |
| --- | --- |
| [lean4export](https://github.com/leanprover/lean4export) | exports a Lean environment as NDJSON (3.1.0); naming constants emits only their transitive closure |
| [nanoda_lib](https://github.com/ammkrn/nanoda_lib) | an independent Lean kernel type-checker in Rust, building for riscv32im essentially unmodified |
| RISC Zero zkVM 3.0.6 | execution and proving, with composition |

## The main result

Cost is governed by what a proof *touches*, not by how large the environment is.
Measured across a **168× prelude** (1,265 → 213,144 declarations):

| | Small | Large | Ratio |
| --- | ---: | ---: | ---: |
| Selected leaves | 11,635 | 11,999 | 1.03x |
| **Stage 2 total** | **62.97M** | **67.71M** | **1.08x** |

A Merkle-authenticated sparse witness delivers only the records a proof touches. This
is what makes all of Mathlib usable as a fixed public prelude — which is also what
makes the scheme private, since a large prelude is the anonymity set an individual
proof hides in.

Five theorems from `examples/mathlib/Practical.lean` are proved against one shared
214,101-declaration prelude; receipts are in `examples/mathlib/proofs/`. Full numbers,
including on-chain gas and the zero-knowledge caveats, are in
**[docs/results.md](docs/results.md)**.

## Documentation

| | |
| --- | --- |
| [design](docs/design.md) | why only the kernel; the two stages; the public/private split |
| [soundness](docs/soundness.md) | three holes that open when a proof is hidden, and the trust assumptions that remain |
| [results](docs/results.md) | every measurement: proofs, scaling, performance, gas, limitations |
| [implementation](docs/implementation.md) | setup, tools, reproducible builds, the nanoda fork |

## Setup

```sh
curl -sSf https://sh.rustup.rs | sh -s -- -y                   # Rust
curl -L https://risczero.com/install | bash && rzup install    # RISC Zero
scripts/setup.sh                                               # exporter + full build
```

## Usage

```sh
# prover
compose public.bin full.bin <theorem> --prove --assume-prelude --out receipt.bin

# verifier — receipt.bin is the only thing received from the prover
checkprelude public.bin                                        # once, no Lean needed
verify receipt.bin public.bin expected.ndjson <theorem>
```

`examples/preimage/run.sh` and `verify.sh` run this end to end;
`examples/zkvm-hello/` is a five-second introduction with no Lean involved.

## Layout

```
lean/              sample theorems for benchmarking
examples/          preimage, Mathlib theorems, and a minimal zkVM example
forks/nanoda_lib/  research fork of the kernel (tracked here, reviewable)
zkvm/
  stmt/            canonical statement digests, pinned type-checking policy
  methods-env/     Stage 1 (envcheck)
  methods/         Stage 2 (thmcheck) and baselines
  host/            drivers and verifier tools
contracts/         on-chain verification and gas measurement
scripts/           setup, export, benchmarks
docs/              design, soundness, results, implementation
```

## What this does not do

- **It is not faster than `lake build`** (25 ms against minutes) and is not meant to
  be. ZK pays off when verifiers are numerous, cannot trust the prover, or are not
  human.
- **The trusted base does not shrink; it moves.** Trusting Lean is replaced by
  trusting nanoda_lib and RISC Zero.
- **Implementations cannot be hidden.** If a statement mentions `myImpl`, its
  definition is public. What can be hidden is why something is true, never what.
- **Succinct is not zero-knowledge**, and in RISC Zero the distinction depends on the
  receipt type. See [results](docs/results.md).
