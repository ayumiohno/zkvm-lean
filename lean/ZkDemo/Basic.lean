/-
Sample theorems for benchmarking.
To minimise prelude dependencies, there is a "closed" world with a hand-defined Nat
alongside one using the standard Nat, so their zkVM costs can be compared.
-/
namespace ZkDemo

/-! ## L0: a hand-defined Nat (minimal prelude dependencies) -/

inductive MyNat where
  | zero : MyNat
  | succ : MyNat → MyNat

def MyNat.add : MyNat → MyNat → MyNat
  | n, .zero    => n
  | n, .succ m  => .succ (MyNat.add n m)

theorem mynat_add_zero (n : MyNat) : MyNat.add n .zero = n := rfl

theorem mynat_add_succ (n m : MyNat) :
    MyNat.add n (.succ m) = .succ (MyNat.add n m) := rfl

/-! ## L1: induction, which brings in the recursor -/

theorem mynat_zero_add : ∀ n : MyNat, MyNat.add .zero n = n
  | .zero     => rfl
  | .succ n   => congrArg MyNat.succ (mynat_zero_add n)

/-! ## L2: the standard Nat and the kernel's Nat extension (the GMP fast path) -/

theorem nat_lit_add : 2 + 2 = 4 := rfl

theorem nat_lit_big : 123456789 * 987654321 = 121932631112635269 := rfl

end ZkDemo
