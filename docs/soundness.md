# Soundness

Hiding the proof gives the prover room to lie. Three holes have to be closed.

## Hole 1: injected axioms

One line — `axiom cheat : False` — in the private part "proves" anything.

**A successful `lake build` guarantees nothing here.** Verified directly:

```lean
axiom claim_axiom : ∃ x : Nat, x < 100000 ∧ f x = 4630
theorem claim_sorry : ∃ x : Nat, x < 100000 ∧ f x = 4630 := sorry
```
```
warning: declaration uses `sorry`
--- exit=0 ---   ← the build passes
```

Any framing of this problem as "prove that the build passed" walks into this.

### Fix: pin the policy inside the guest

An allowlist is not enough on its own. **The policy must not come from a private
input.** nanoda's `axiom_permitted` reads:

```rust
fn axiom_permitted(&self, n: NamePtr) -> bool {
    self.config.unsafe_permit_all_axioms ||        // short-circuits
        self.config.permitted_axioms.as_ref().map(|v| v.contains(..)).unwrap_or(false)
}
```

The mutual-exclusion check that forbids combining `unsafe_permit_all_axioms` with an
allowlist lives only in `Config::try_from(&Path)`, and **does not run** on the
`serde_json::from_str` path a guest would use. So if the config arrived by witness, a
malicious prover could keep the same guest image and simply send:

```json
{"permitted_axioms": null, "unpermitted_axiom_hard_error": false,
 "unsafe_permit_all_axioms": true}
```

**A safe allowlist held on the host is not a security boundary when the host is
untrusted.** The policy is therefore fixed in `zkvm/stmt/src/policy.rs`, and the guest
asserts at startup that `unsafe_permit_all_axioms` is false, that `num_threads == 1`,
and that the allowlist has the expected contents. `nat_extension` and
`string_extension` are likewise not taken from the witness.

**Negative test.** `ZkDemo.bogus` derives `1 = 2` from `axiom cheat : False`:

```
thread 'main' panicked at src/tc.rs:230:
declaration not found in infer_const, ZkDemo.cheat
exit=101
```

Inside the zkVM a panic means no proof is produced, so this fails safe.

## Hole 2: statement substitution

The constants a statement mentions might themselves be malicious. With
`def Even (n) := True`, "every number is even" becomes provable.

### Fix

The export is split into a public prefix reachable from the statement and a private
suffix holding the proof term, and two things are committed to the journal:

- **the hash of the public prefix** — the verifier reads the public part in the clear
  and confirms nothing was substituted;
- **the canonical statement digest** — what was proved, without showing the proof.

The split was measured (`examples/preimage/run.sh`):

| natVal | public (`f`) | full | Origin |
| --- | ---: | ---: | --- |
| `1000003` | 1 | 1 | `f`'s definition (public) |
| `100000` | 0 | 1 | the statement's bound (public) |
| `4630` | 0 | 1 | the statement's target (public) |
| **`31337`** | **0** | **1** | ★ the secret, inside the proof term |

### The statement digest alone is not enough

**The canonical digest includes constants' *names* but not their definitions.** If
`Preimage.f` matches by name while the prover substituted its body, the digest can
still agree. Closing hole 2 requires comparing the **prefix digest** against the
verifier's own expectation — which is what `verify` does.

## The independent verifier

`verify <receipt.bin> <public.bin> <expected export.ndjson> <theorem name>` checks
**all** of the following. Dropping any one opens a hole.

`--expected-name <n>` names the declaration to compare against inside the expected
export when it differs from the journal's name, and `--declarations <n>` is how large
a private suffix is accepted (default 1).

| | Check | If missing |
| --- | --- | --- |
| 1 | the receipt is valid for `THMCHECK_ID` | a proof of a different program is accepted; also rejects conditional receipts |
| 2 | the journal's env image ID is the official `ENVCHECK_ID` | a malicious Stage 1 could declare any count and skip real checks |
| 3 | the prelude digest matches the verifier's `public.bin` | **substituted definitions go undetected (hole 2)** |
| 4 | the theorem name is the expected one | a proof of a different theorem is accepted |
| 5 | the statement digest matches one computed from the verifier's own proposition | a proof of a different proposition is accepted |
| 6 | no axioms outside the allowlist | hole 1 |
| 7 | the private suffix cannot have introduced an allowlisted axiom — either the public environment declares all of them with their pinned types, or the suffix holds nothing but the theorem | an allowlisted **name** the prelude does not declare can be declared by the suffix (`axiom Lean.trustCompiler : ∀ p, p`), and the parser admits an axiom on its name alone |
| 8 | every constant the statement mentions is defined by the public environment | a statement digest pins constants by name, not by definition, so a name absent from the prelude is resolved by the prover's suffix (`MyClaim := True`) |
| 9 | the verifier's own export and the public environment agree, declaration by declaration, on everything they share | check 8 only makes the names public; if the verifier wrote their proposition against a different environment, `MyClaim := hard` in their file and `MyClaim := True` in the prelude still produce the same digest |

The prover sends **only the receipt**. The verifier supplies the public environment
they trust and the proposition they wrote themselves (proof `sorry`).

```
$ verify out/mynat_receipt.bin out/mn_pre.bin out/expect.ndjson ZkDemo.mynat_add_zero
✅ 1. receipt valid (image id 23370ed42978e0cb...)
✅ 2. Stage 1 image id is the official one
✅ 3. prelude matches the verifier's public environment (4394 bytes)
✅ 4. theorem name matches: ZkDemo.mynat_add_zero
✅ 5. statement digest matches: ac5f832203dbba30...
✅ 6. no axioms outside the allowlist

Verified. 22 declarations type-check.
The proof term was never seen.
```

The verifier's own export (`Expect.what_i_want : ∀ n, MyNat.add n .zero = n := sorry`)
does not even contain the name `mynat_add_zero`. **Equal types, equal digest.**

### Tamper detection (negative tests)

| Tampering | Result |
| --- | --- |
| expect a different prelude | ❌ fails at 3 |
| expect a different proposition | ❌ fails at 5 |
| change the theorem name | ❌ fails at 4 |
| corrupt one byte of the receipt | ❌ fails at 1 (`proof is invalid`) |
| use an axiom outside the allowlist | ❌ the guest panics; no proof exists |

## Hole 3': the prover choosing Stage 1's image ID

If `env::verify(image_id, journal)` took `image_id` from a private input, the prover
could supply a malicious Stage 1 whose journal declares any number of checked
declarations. Stage 2 skips that many via `check_declars_skipping(n)`, so **the
theorem's own declarations could go unchecked**.

### Fix: pin it at build time

The `methods-env` crate is built first and `methods/build.rs` writes its image ID to a
file; thmcheck pulls it in with `include!(env!(...))`.

```rust
// methods/thmcheck/src/main.rs
include!(env!("PINNED_ENVCHECK_ID_FILE"));
...
env::verify(Digest::from(PINNED_ENVCHECK_ID), env_journal.as_slice())
```

The guest accepts only that ID, so the witness cannot move it. The pinned value also
goes in the journal, so the verifier can check it too.

## Hole 3: bypassing the kernel

`native_decide`, `implemented_by` and `unsafe` do not go through the kernel, and using
them empties out the meaning of "it type-checks". lean4export omits unsafe
declarations by default (`--export-unsafe` enables them explicitly); not using
`native_decide` is an operational condition that has to be stated.

## The environment trust mode is always published

| | That the prelude type-checks is |
| --- | --- |
| `certified` | proved by Stage 1 inside the zkVM |
| **`assumed`** (recommended default) | **assumed; the verifier confirms it with `checkprelude`** |

In `assumed` the Stage 1 image id is zeroed, and `verify --require-certified` rejects
it. The environment descriptor the guest receives as a public input is not trusted:
all of it reaches the journal and is compared against values the verifier computes
from their own prelude.

## "Was the program that ran really a type-checker?" cannot be proved

A receipt attests to exactly this:

> a **binary** with image id `X` terminated normally and produced this journal.

The image id is a hash of the guest binary, so *which* binary ran is pinned. But the
receipt says nothing about **what that binary does**. Cryptography cannot distinguish
a Lean kernel from a program that just prints a journal.

Two things narrow the gap.

### 1. Reproducible builds — established with Docker

If a verifier builds from source and gets the same image ID, then image ID and source
are mechanically linked.

**Local builds do not achieve this.** The ELF embeds absolute paths in 42 places
(file names for panic messages), so moving the checkout changes the ID.

```
$ strings thmcheck | grep /Users/ayumi | head -2
bad env descriptor/Users/ayumi/Downloads/lean-zkvm/forks/nanoda_lib/src/level.rs
/Users/ayumi/Downloads/lean-zkvm/zkvm/stmt/src/merkle.rs

built in /Users/ayumi/Downloads/lean-zkvm : ad39f970...
built in /tmp/repro                       : 5f309a43...   ❌ differs
```

`--remap-path-prefix` needs the real path on its left-hand side, so it cannot be
written into the guest's `Cargo.toml` or `GuestOptions`. Fixing the path *inside a
container* is the only option.

`LEAN_ZKVM_DOCKER=1` switches to a Docker build. Reproducibility was confirmed along
two axes:

| Axis | Result |
| --- | --- |
| same machine, different path | ✅ identical |
| **arm64 container vs amd64 container** | ✅ identical (the ELF matches byte for byte) |

```
                  arm64 container       amd64 container
envcheck_id  10403914b086d0eae6...  10403914b086d0eae6...  ✅
thmcheck_id  e3c5513d2cf6b0a39e...  e3c5513d2cf6b0a39e...  ✅
```

The Docker ELF contains **zero** host absolute paths. Host OS differences
(macOS ↔ Linux) are untested, but the build is self-contained in the container and it
already matched across architectures.

**Local and Docker builds produce different image IDs.** Mixing them fails
verification, so anything published must be built with Docker.

### 2. Reading the source — this part is human

| Code to read | Lines |
| --- | ---: |
| `thmcheck/src/main.rs` | 236 |
| `thmcheck/src/io.rs` | 16 |
| `stmt/src/` (policy, Merkle, digests) | 733 |
| **`nanoda_lib/src/` (the kernel)** | **9,408** |
| **Total** | **10,393** |

For comparison, Lean's own C++ kernel is around ten thousand lines too.

### The chain of trust

```
receipt
  ↓ cryptography (STARK)        ← mathematically guaranteed
image id
  ↓ reproducible build (Docker) ← mechanically checkable (confirmed)
10,393 lines of source
  ↓ human review                ← ★ the only trusted step
"this correctly implements the Lean kernel"
```

ZK cannot fill in ★. **You do not stop trusting Lean; you trade trusting Lean for
trusting 9,408 lines of nanoda plus RISC Zero.**

## Trust assumptions

A zkVM guarantees that a computation was executed correctly, not that it was the right
computation. A bug in the guest means **the bug is proved to have executed correctly**.

| | Status |
| --- | --- |
| **nanoda_lib correctly implements the Lean kernel** | not formally verified |
| **The RISC Zero proof system is sound** | 97-bit conjectured security |
| (in `assumed` mode) **that prelude type-checks** | the verifier confirms it with `checkprelude` — 0.08 s, no Lean needed |

[lean4lean](https://github.com/digama0/lean4lean) aims to prove kernel correctness in
Lean itself, which would reduce the first assumption.

## The host is not trusted

`tobin` (NDJSON → binary) and `prune` (witness pruning) run outside the zkVM without
adding trust assumptions. The guest does not guarantee that the binary it received
faithfully represents the original NDJSON. It guarantees only that it type-checked the
terms that binary denotes and derived the statement digest from them. A lying host
produces a digest that does not match what the verifier expects.

**Meaning is pinned by the commitments**, so the guest needs only memory safety, plus
determinism of the conversion so the verifier can reproduce the same hash.

### Attacks specific to the sparse environment

Because Stage 2 never receives the whole prelude, two more paths must be closed.

| Attack | Guest-side rejection |
| --- | --- |
| tampered records, or records from another environment | the compact multiproof does not match Stage 1's record Merkle root |
| supplying an authenticated record's dependency from the private suffix | prelude index remapping consults only the authenticated prelude set |
| redefining an omitted public declaration in the suffix | Stage 1 commits to the public name set; Stage 2 checks disjointness |
| incomplete dependency hints | the real kernel lookup is not in Stage 1's checked name set and fails |

Record payloads are never handed to the type-checker before their Merkle proof
verifies. Index compaction happens deterministically after authentication, and
unselected records are not materialised even as placeholders.

## Remaining leakage

- **Proof size.** Segment count and proving time both correlate with it. Compressing
  to a succinct receipt removes the segment-count channel; proving time remains
  (6.8 s versus 197.2 s distinguishes the hidden term's size).
- **Zero-knowledge depends on the receipt type.** `ZK_CYCLES` padding is injected in
  `risc0-circuit-recursion` only; `risc0-circuit-rv32im` injects none, so a composite
  receipt is succinct but not zero-knowledge. See [results](results.md).

## A note on terminology

**Succinct ≠ zero-knowledge.** "Cheap to verify" and "reveals nothing" are different
properties, and only some receipt types provide the second.
