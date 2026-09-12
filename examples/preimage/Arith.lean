/-
A controlled experiment separating the cost of arithmetic itself from the cost of
unfolding the recursor (brecOn).

`iter 10 x` applies f ten times, unfolding Nat.brecOn once per application.
`f10 x` writes the same ten applications out without recursion.
The amount of arithmetic is identical; only recursor unfolding differs.
-/
import Preimage
open Preimage

namespace Arith

/-- Apply f ten times without recursion. -/
def f10 (x : Nat) : Nat := f (f (f (f (f (f (f (f (f (f x)))))))))

set_option maxRecDepth 100000

/-- The recursion-free version; the same amount of arithmetic as iter10. -/
theorem knows_f10 : ∃ x : Nat, x < 100000 ∧ f10 x = 96524 :=
  ⟨31337, by decide, by decide⟩

end Arith
