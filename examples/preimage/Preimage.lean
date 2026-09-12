/-
Stating "I know a preimage" as a Lean theorem.

  public: `f`'s definition and the theorem's statement
  secret: the `31337` occurring inside the proof term

A proof of `∃ x, P x` literally contains `x`, so hiding the proof *is* hiding the
secret. That is what makes this shape fit a zkVM.
-/
namespace Preimage

/-- PUBLIC. A vaguely one-way function. -/
def f (x : Nat) : Nat := (x * x + 7) % 1000003

/-- PUBLIC. Iterate `f` `n` times; `n` dials the weight of the proof. -/
def iter : Nat → Nat → Nat
  | 0,     x => x
  | n + 1, x => f (iter n x)

set_option maxRecDepth 100000

/-! ## Claim 1: knowing a preimage of `f` -/

/-- There is an `x < 100000` with `f x = 4630`.
    (`31337` is the only preimage, and it does not appear in the statement.) -/
theorem knows_preimage : ∃ x : Nat, x < 100000 ∧ f x = 4630 :=
  ⟨31337, by decide, by decide⟩
--  ^^^^^ SECRET. Appears only in the proof term.

/-! ## Claim 2: raising the iteration count to load the kernel (benchmarking) -/

theorem knows_preimage_iter10 : ∃ x : Nat, x < 100000 ∧ iter 10 x = 96524 :=
  ⟨31337, by decide, by decide⟩

theorem knows_preimage_iter100 : ∃ x : Nat, x < 100000 ∧ iter 100 x = 528168 :=
  ⟨31337, by decide, by decide⟩

theorem knows_preimage_iter1000 : ∃ x : Nat, x < 100000 ∧ iter 1000 x = 856612 :=
  ⟨31337, by decide, by decide⟩

end Preimage
