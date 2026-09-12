/-
The same proposition written two ways — brute computation versus a structural
argument — to see how much the kernel's checking cost changes.

An optimisation axis independent of the zkVM side (composition, binary format, Merkle).
-/
namespace Style

/-! ## Proposition A: a concrete equation, where little but `decide` applies -/

def big : Nat := 123456 * 654321 + 7

/-- Brute computation: the kernel really multiplies. -/
theorem big_eq_decide : big = 80779853383 := by decide

/-- `rfl` also makes the kernel compute, but with less machinery around it. -/
theorem big_eq_rfl : big = 80779853383 := rfl

/-! ## Proposition B: a universal, where either approach is available -/

/-- Structural: induction. The kernel computes nothing. -/
theorem add_zero_struct : ∀ n : Nat, n + 0 = n := fun _ => rfl

/-- Brute-force a bounded universal: make the kernel check all 100 cases. -/
theorem lt_bounded_decide : ∀ n : Nat, n < 100 → n < 200 := by
  intro n h
  exact Nat.lt_trans h (by decide)

/-- The same thing, shown structurally. -/
theorem lt_bounded_struct : ∀ n : Nat, n < 100 → n < 200 :=
  fun _ h => Nat.lt_trans h (Nat.lt_of_sub_eq_succ rfl)

end Style
