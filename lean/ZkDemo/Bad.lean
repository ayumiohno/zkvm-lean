/-
A soundness negative test: derive a false theorem from an axiom outside the
allowlist. Not imported from ZkDemo.lean, so the normal build stays clean.
-/
namespace ZkDemo
axiom cheat : False
theorem bogus : 1 = 2 := cheat.elim
end ZkDemo
