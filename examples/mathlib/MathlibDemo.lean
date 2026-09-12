/-
Measuring what proof terms built by Mathlib tactics (ring, linarith, nlinarith) cost
in the zkVM's kernel check.

The tactics themselves run during host-side elaboration and never enter the zkVM.
What matters is the **generated proof term** and the size of its dependency closure.

Statements are named `Prop`s so public and private can be split: exporting with
`lean4export MathlibDemo -- MathlibDemo.stmt_x MathlibDemo.thm_x` in that order puts
the statement's closure in the public prefix and the proof term in the private
suffix.
-/
import Mathlib.Tactic.Ring
import Mathlib.Tactic.Linarith
import Mathlib.Data.Int.Basic
import Mathlib.Data.Rat.Defs
import Mathlib.Data.Real.Basic

namespace MathlibDemo

/-! ## ring: normalisation over a commutative ring -/

/-- PUBLIC. A claim over the integers. -/
def stmt_int (x y : ℤ) : Prop := (x + y) ^ 2 = x ^ 2 + 2 * x * y + y ^ 2
/-- SECRET. The proof term. -/
theorem thm_int : ∀ x y : ℤ, stmt_int x y := by intro x y; unfold stmt_int; ring

/-- PUBLIC. Over the rationals. -/
def stmt_rat (x y : ℚ) : Prop := (x + y) ^ 2 = x ^ 2 + 2 * x * y + y ^ 2
theorem thm_rat : ∀ x y : ℚ, stmt_rat x y := by intro x y; unfold stmt_rat; ring

/-- PUBLIC. Over the reals — the heaviest, since it drags in their construction. -/
def stmt_real (x y : ℝ) : Prop := (x + y) ^ 2 = x ^ 2 + 2 * x * y + y ^ 2
theorem thm_real : ∀ x y : ℝ, stmt_real x y := by intro x y; unfold stmt_real; ring

/-! ## linarith / nlinarith: certificate-based -/

def stmt_lin (x : ℚ) : Prop := 2 * x + 1 ≤ 5 → x ≤ 2
theorem thm_lin : ∀ x : ℚ, stmt_lin x := by
  intro x; unfold stmt_lin; intro h; linarith

def stmt_nlin (x : ℝ) : Prop := 0 < x → 0 < x ^ 2
theorem thm_nlin : ∀ x : ℝ, stmt_nlin x := by
  intro x; unfold stmt_nlin; intro h; nlinarith

end MathlibDemo
