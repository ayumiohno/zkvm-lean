# Proving knowledge of a preimage

Prove you know an `x` with `f x = 4630`, without revealing `x`.

```sh
./run.sh      # where the secret appears, plus cycle counts
./verify.sh   # public/private split: verify while the proof term stays hidden
```

## Why an existential

Only the proof term can be hidden; the statement is necessarily public. So this is
useful exactly when the secret lives inside the proof term — which is what a proof of
`∃ x, P x` gives you.

```lean
theorem knows_preimage : ∃ x : Nat, x < 100000 ∧ f x = 4630 :=
  ⟨31337, by decide, by decide⟩
--  ^^^^^ appears only in the proof term
```

Exporting `f`'s dependency closure and the whole theorem separately shows where each
number lands:

| natVal | public (`f`) | full | Origin |
| --- | ---: | ---: | --- |
| `1000003` | 1 | 1 | `f`'s definition (public) |
| `100000` | 0 | 1 | the statement's bound (public) |
| `4630` | 0 | 1 | the statement's target (public) |
| **`31337`** | **0** | **1** | ★ the secret, inside the proof term |

In the full export `31337` is a single `Expr` node: `{"ie":5524,"natVal":"31337"}`.
Isolating it in the witness is the whole point.

## Tuning proof weight

`iter n` applies `f` n times, so the kernel's workload can be dialled freely while the
export stays the same size (executor, no proving):

| Theorem | bytes | Declarations | parse | check | total |
| --- | ---: | ---: | ---: | ---: | ---: |
| `knows_preimage` | 376,335 | 225 | 44.8M | 73.8M | **175M** |
| `knows_preimage_iter10` | 380,599 | 228 | 45.3M | 76.7M | **180M** |
| `knows_preimage_iter100` | 380,602 | 228 | 45.3M | 94.9M | **198M** |
| `knows_preimage_iter1000` | 380,604 | 228 | 45.3M | 296.6M | **399M** |

One application of `f` costs about 220K cycles inside the kernel. Because the export is
fixed at 380 KB, this isolates computational weight from input size. Even the lightest
case costs 175M cycles, almost all of it the transitive closure of `Nat`, `%` and
`Decidable` — 225 declarations. (These are pre-optimisation figures; see
[docs/results.md](../../docs/results.md).)

## Caveats

- `f` is quadratic, so `31337` is easy to recover from `4630`. **This demonstrates the
  mechanism, not cryptographic hardness.** Raise `iter` if hardness is wanted.
- `by decide` goes through the kernel and is sound. `native_decide` bypasses it and
  must not be used.
