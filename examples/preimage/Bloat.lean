/-
Definitions that exist only to enlarge the environment. They have nothing to do with
the theorem, but pulling in List, Array, String and friends inflates the prelude.

A control group for measuring whether the number of declarations touched while
checking a theorem grows with the environment.
-/
import Preimage

namespace Bloat

def payload : String :=
  let xs := (List.range 10).map (· * 2)
  let a := xs.toArray
  s!"{a.size} {xs.length} {String.join (xs.map toString)}"

end Bloat
