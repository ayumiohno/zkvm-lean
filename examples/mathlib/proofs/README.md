# Receipts for real Mathlib theorems

Real zk proofs, generated on GPU, for the theorems in `../Practical.lean`. A verifier
needs only a `receipt_*.bin` and a public prelude they built themselves.
**The proof terms are not here.**

| Theorem | Statement digest | Cycles | Proving |
| --- | --- | ---: | ---: |
| `thm_infinitude` infinitely many primes | `7e349da5f14ac421fea6d5395e63ed867fe4ccc4e81d763e52da1fabb9f08aa4` | 2,359,296 | 6.8 s |
| `thm_sqrt2` √2 is irrational | `c7ed515d09ad8c2b792b703eb7fccdd807d1a4408d00d2a26b7eac26de6d5919` | 3,407,872 | 10.1 s |
| `thm_gauss` Gauss sum | `9e8cd68f37ba21e92a0c2d8ac5428e0136648cd3d9431a05523a0142414c7649` | 4,718,592 | 12.9 s |
| `thm_factor` knowing a factorisation of 3233 | `4eab9efc745fe90173ebe498ed8df2c02d43ea47f87755ba783cac581c39a6ea` | 37,748,736 | 95.2 s |
| `thm_amgm` AM–GM over ℝ | `d52356f2782bbd930c47e0da3d6ea167931aad41ae940d55d1647b30a9119dda` | 77,725,696 | 197.2 s |

Quadro RTX 8000, succinct, `--assume-prelude`. Each reports `declarations checked: 1`
and passes all six `verify` checks. Cycles are segment-padded lengths, which is what
proving time tracks.

`receipt_*_g16.bin` are the same proofs wrapped in Groth16 for on-chain use: 1,473–1,481
bytes against 224,210. `receipt_infinitude_g16_alt.bin` is a second, independently
randomised wrapping of the same proof — both verify.

`thm_factor` proves `∃ p q, p.Prime ∧ q.Prime ∧ p ≠ q ∧ p * q = 3233`. **61 and 53
appear nowhere in the receipt**; they exist only in the proof term.

## Verifying

```sh
# Write the proposition yourself and export it. Export the theorem (thm_*), not the
# statement abbreviation (stmt_*), whose type is Prop and will not match.
lake env <lean4export> Mathlib Practical -- Practical.thm_infinitude > expected.ndjson

verify receipt_infinitude.bin public.bin expected.ndjson Practical.thm_infinitude \
       --manifest ../../../demo/manifest.json
```

Image IDs come from the Docker build (`scripts/build-release.sh`):

```
envcheck  4dbcd2fd7590c4a7d9c3974854c6d6949f11e093a77787fdf968c1c007cc0c63
thmcheck  44088753d3612b3b5641daf6a7af9f962211854afe91693183d3719d1e1350c7
```

## The public prelude is not included

All five proofs share one prelude, too large (683 MB) to track here:

```
size          682,879,924 bytes
declarations       214,101
records         37,277,329
SHA-256       7d539df7b3e795c7f7bd7ea1327687cd3f5083fdd958419ce8a14028653e89a1
Merkle root   1194e86945579ca116c57c917f25c1b8ffd523998732049cf9d974ed3c36a963
```

Export is deterministic, so `roots.txt` plus these pins reproduce it byte for byte:

| Pinned | |
| --- | --- |
| mathlib | `e21ec05048292b3de86d4cf1987e2208171a5642` |
| lean-toolchain | `leanprover/lean4:v4.34.0-rc2` |
| lean4export | `411dce7db58a3afc60ecab2d211acd1042b593dc` |
| root constants | `roots.txt` (11,492) |

```sh
cd examples/mathlib
ROOTS=$(tr '\n' ' ' < proofs/roots.txt)
lake env <lean4export> Mathlib Practical -- $ROOTS > public.ndjson
tobin public.ndjson public.bin
sha256sum public.bin        # should match 7d539df7...
```

## What is assumed

With `--assume-prelude`, that those 214,101 declarations type-check is an **assumption,
not a proof**; `verify` reports it as `⚠️ environment is assumed`. A verifier can
discharge it without Lean:

```
$ checkprelude public.bin
✅ all 214101 declarations type-check (442.81s)
✅ no axioms outside the allowlist
```

Seven and a half minutes, once, covering every proof against this prelude. See
[docs/results.md](../../../docs/results.md).
