/-
Measuring proving cost on real Mathlib theorems.

Where `MathlibDemo` examines the cost of **tactics** like `ring` and `linarith`, this
collects propositions people actually want to prove.

Statements are named `Prop`s so public and private can be split: exporting with
`lean4export Mathlib Practical -- <public roots> Practical.thm_x` in that order puts
the statement's closure in the public prefix and the proof term in the private suffix.

The content of a proof — which lemmas were combined and how — stays in the zkVM's
witness; only the statement digest reaches the journal.
-/
import Mathlib

namespace Practical

/-! ## 1. There are infinitely many primes (Euclid) -/

/-- PUBLIC. For every `n` there is a prime at least that large. -/
def stmt_infinitude : Prop := ∀ n : ℕ, ∃ p, n ≤ p ∧ Nat.Prime p
/-- SECRET. The proof term. -/
theorem thm_infinitude : stmt_infinitude := fun n => Nat.exists_infinite_primes n

/-! ## 2. √2 is irrational -/

/-- PUBLIC. -/
def stmt_sqrt2 : Prop := Irrational (Real.sqrt 2)
theorem thm_sqrt2 : stmt_sqrt2 := irrational_sqrt_two

/-! ## 3. The Gauss sum -/

/-- PUBLIC. Twice 0 + 1 + ... + (n-1) is n(n-1). -/
def stmt_gauss : Prop := ∀ n : ℕ, (∑ i ∈ Finset.range n, i) * 2 = n * (n - 1)
theorem thm_gauss : stmt_gauss := fun n => Finset.sum_range_id_mul_two n

/-! ## 4. Knowing a factorisation — the shape that matters for ZK

Prove that you know the prime factorisation of 3233 **without revealing the factors**.
Only 3233 appears in the statement; 61 and 53 exist solely in the proof term. -/

/-- PUBLIC. 3233 is a product of two distinct primes. -/
def stmt_factor : Prop := ∃ p q : ℕ, p.Prime ∧ q.Prime ∧ p ≠ q ∧ p * q = 3233
/-- SECRET. 61 * 53 — the thing being hidden. -/
theorem thm_factor : stmt_factor :=
  ⟨61, 53, by norm_num, by norm_num, by norm_num, by norm_num⟩

/-! ## 5. An inequality over the reals -/

/-- PUBLIC. AM–GM in two variables, squared form. -/
def stmt_amgm : Prop := ∀ a b : ℝ, 2 * a * b ≤ a ^ 2 + b ^ 2
theorem thm_amgm : stmt_amgm := fun a b => two_mul_le_add_sq a b

end Practical
