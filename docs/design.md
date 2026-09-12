# Design

## The problem

To convince someone that a Lean 4 theorem and its proof are correct, they normally
have to install Lean and run `lake build` themselves. And if the proof must stay
private, verification cannot happen at all.

This project proves, **without revealing the proof term**, that a theorem passes Lean's
kernel type-checker. The verifier needs no Lean installation — only a zk proof to check.

## The central decision: only the kernel goes in the zkVM

`lake build` includes the elaborator, macro expansion and the tactic framework. None
of that is viable inside a zkVM. But Lean's trusted computing base is **the kernel
alone**: whatever tactics produced a proof term, the theorem holds if the kernel
type-checks that term. This is the de Bruijn criterion.

So type-checking is the only thing that needs to run in the zkVM.

```
┌─ Host (outside the zkVM, untrusted) ──────────────────┐
│  Preimage.lean                                        │
│    def f (x : Nat) := (x*x+7) % 1000003     public    │
│    theorem knows_preimage : ∃ x, ... :=               │
│      ⟨31337, by decide, by decide⟩          secret    │
│         ↓ lake build (elaboration, tactics)           │
│    proof term = Exists.intro 31337 (And.intro ...)    │
│         ↓ lean4export → tobin                         │
│  public.bin + private suffix                          │
└────────────────────────┬──────────────────────────────┘
                         ↓
┌─ zkVM guest ──────────────────────────────────────────┐
│  the nanoda kernel type-checks                        │
│  failure ⇒ panic ⇒ no proof is produced               │
└────────────────────────┬──────────────────────────────┘
                         ↓
                  zk proof + journal
```

## Components

| | Role |
| --- | --- |
| [lean4export](https://github.com/leanprover/lean4export) | Writes a Lean environment as NDJSON (format 3.1.0). Naming constants emits only their transitive closure. |
| [nanoda_lib](https://github.com/ammkrn/nanoda_lib) | An independent Lean kernel type-checker in Rust. Builds for riscv32im essentially unmodified. |
| RISC Zero zkVM 3.0.6 | Execution environment, with composition (`env::verify`). |

nanoda_lib was chosen because it is Rust. Lean's own kernel (C++ and Lean) cannot be
put in a zkVM.

## Two stages (composition)

Most of the cost is type-checking the prelude — the transitive closure of `Nat`,
`Decidable` and so on — which does not need to be paid per theorem.

```
Stage 1 (envcheck)   type-check the prelude closure → receipt_1
                     commits to the prelude hash, record root, checked names
        ↓ env::verify() inside the guest
Stage 2 (thmcheck)   authenticate the needed records by multiproof, parse them,
                     and check that real references fall inside Stage 1's set
```

In full mode Stage 2 compares the prelude SHA-256; in sparse mode it verifies
membership against the record Merkle root Stage 1 committed to. It also commits to the
set of public declaration names, so a private suffix cannot shadow an omitted public
declaration by redefining it under the same name.

The host that selects dependencies is not part of the TCB. Stage 1 builds the
dependency closure to a fixed point from nanoda's real lookups, and Stage 2 checks
containment against real lookups too, so an incomplete dependency hint fails safe.

**Stage 1 is optional.** What it proves — that Lean's standard prelude type-checks —
is a public, fixed fact. A verifier who is willing to establish that another way can
skip Stage 1 entirely.

## How the environment is established

Stage 2 skips checking the prelude (`check_declars_skipping`), which needs
justification. **"Public" and "type-checks" are different properties.** Public means
anyone *can* check it, not that anyone *has*. An unchecked prelude could contain
`theorem everything_is_false : False := <ill-typed body>`, and everything follows.

| | The verifier needs | Trusts |
| --- | --- | --- |
| A. Verifier runs `lake build` | **a Lean installation** + hours | nobody |
| **B. Verifier runs nanoda natively** | **`checkprelude`, one Rust binary** | nanoda |
| C. Take someone's word | nothing | **that person** |
| D. Stage 1 zk proof (`certified`) | 0.06 s of receipt verification | nanoda + STARK |

### B is the default

```
$ checkprelude out/mlp3_int.bin
  SHA-256     : 738895c3c12ea09e...
  Merkle root : 7a8ad8fa285bca9b...
✅ all 1265 declarations type-check (0.08s)
✅ no axioms outside the allowlist
```

**1,265 declarations in 0.08 s.** The same check inside the zkVM is 1,398M cycles —
about 65 hours on CPU. Done once, it covers every proof against that prelude.

### Stage 1 only pays off on chain

Stage 1 costs roughly **1.09M cycles per declaration**: about 63 billion cycles for
Init's 58,143 declarations, and about 540 billion for Mathlib's 497,421 non-internal
constants. At the measured 370K cycles/s on GPU that is 2 days and 17 days
respectively. It is a public good — produced once, reused by everyone — so the
magnitude is not absurd, but the current Stage 1 holds the whole environment in one
guest and runs out of memory.

**If the verifier can run anything at all, use B.** Stage 1 matters when:

- **verification happens on chain** — a contract cannot run nanoda;
- there are many verifiers and the prelude is large enough that each checking it
  natively is wasteful.

For standard Mathlib even B is arguably unnecessary: that Mathlib type-checks is
confirmed by CI daily and by everyone who builds it.

### Implementation

`compose --assume-prelude` skips Stage 1. The environment descriptor — prelude
SHA-256, Merkle root, record count, hash of the declaration-name set — is passed as a
public input and committed to the journal verbatim.

**This is not trusting the host.** Because the descriptor is in the journal, the
verifier recomputes it from their own prelude and compares. A mismatch fails
verification. The only thing assumed is that the prelude type-checks, and that is
what B establishes.

**The mode is always in the journal.** Without it, a proof that merely assumed the
prelude could be passed off as Stage 1-backed. `verify --require-certified` rejects
`assumed`.

## The public / private split

Hiding the proof term alone conveys nothing. The verifier also has to learn **what was
proved**.

| | Public | Secret |
| --- | --- | --- |
| Prelude | ✅ the verifier holds all of it; the zkVM gets an authenticated subset | |
| The theorem's type (statement) | ✅ canonical digest in the journal | |
| Axioms used | ✅ anything outside the allowlist is in the journal | |
| **Proof term and helper lemmas** | | ✅ **stays in the witness** |

A *canonical* statement digest is needed because terms in an export are shared by
integer index, so the same proposition gets different indices in different files. The
digest is built from term structure alone (`zkvm/stmt/`).

The verifier's workflow:

1. Write the proposition in Lean. No proof is needed — `sorry` is fine.
2. Export it and compute the digest with `stmtdigest`.
3. Compare against the journal.

Because the digest depends only on the type, **the file, the theorem name and the
proof can all differ and the digest still matches**.

## Why the examples are existentials

Only the proof term can be hidden; the statement must be public. So this is useful
exactly when **the secret lives inside the proof term**. In Lean that is what an
existential gives you: a proof of `∃ x, P x` literally contains `x`.

```lean
theorem knows_preimage : ∃ x : Nat, x < 100000 ∧ f x = 4630 :=
  ⟨31337, by decide, by decide⟩
--  ^^^^^ appears only in the proof term
```

In the export it is a single `Expr` node: `{"ie":5524,"natVal":"31337"}`.

### Shared infrastructure is not something to hide

Tactic infrastructure such as `Mathlib.Tactic.Ring` belongs on the **public** side.
Leaving it in the private part made Stage 2 **9.1× more expensive**. What is worth
hiding is which lemmas were combined and how — not the fact that `ring` was available.

**But a prelude must never be tailored per proof.**

| Prelude contents | What leaks |
| --- | --- |
| Exactly the 545 declarations this theorem uses | nearly everything; `ring` is obvious |
| All of `Mathlib.Tactic.Ring` | that `ring` was probably used |
| **All of Mathlib** | **nothing** |

**A prelude defines an anonymity set.** The larger it is, the wider the space of
propositions and techniques it could have supported, and the deeper an individual
proof is buried.

Tailoring is also structurally prevented: `verify` checks the prelude digest against
the verifier's own `public.bin`, so the prelude is forced to be a pre-agreed fixed
object.

This ties privacy directly to performance:

```
larger prelude → wider anonymity set (privacy ↑)
               → more to authenticate and parse (performance ↓)
```

**The sparse witness is what reconciles the two.** It is not only a performance
mechanism; it is what makes a large fixed prelude practical at all.

### What this cannot do

"Keep an implementation secret while proving it correct" is **impossible**. If the
statement is `theorem impl_correct : ∀ x, myImpl x = spec x`, then `myImpl` is
referenced by the statement and must be public.

**What can be hidden is *why* something is true, never *what* is true.**

## Intended uses

If the only motivation is avoiding a toolchain install, this does not pay for itself
(`lake build` in 2.3 s versus minutes of proving). It pays off for:

- **Proving a vulnerability exists** — asserting "this contract is broken" without
  revealing the exploit. A proof of `∃ s, reachable s ∧ ¬ solvent s` *is* the attack.
- **Selling a proof** — showing you have one before handing it over.
- **On-chain verification** — the verifier cannot run `lake build`.
- **Bulk-verifying AI-generated proofs** — where only succinctness is needed, not ZK.
