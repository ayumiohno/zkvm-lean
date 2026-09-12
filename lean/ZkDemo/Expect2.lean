/- The verifier's proposition for the second theorem. They have no proof. -/
import ZkDemo.Basic
namespace Expect2
theorem what_i_want : ∀ (n m : ZkDemo.MyNat),
    ZkDemo.MyNat.add n (.succ m) = .succ (ZkDemo.MyNat.add n m) := sorry
end Expect2
