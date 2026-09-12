/-
Written from the verifier's side.
They have no proof, so `sorry` stands in. The digest is determined by the theorem's
**type** alone, so it is the same whatever the proof is — or whether there is one.
-/
import Preimage
open Preimage

namespace Verifier

/-- The proposition the verifier wants proved. -/
theorem expected : ∃ x : Nat, x < 100000 ∧ f x = 4630 := sorry

end Verifier
