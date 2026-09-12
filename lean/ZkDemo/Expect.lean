/- Written from the verifier's side. They have no proof, so `sorry`. -/
import ZkDemo.Basic
namespace Expect
theorem what_i_want : ∀ n : ZkDemo.MyNat, ZkDemo.MyNat.add n .zero = n := sorry
end Expect
