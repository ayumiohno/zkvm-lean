# A minimal zkVM example

The zkVM mechanics on their own. No Lean involved; the whole round trip takes about
five seconds.

```sh
cargo build --release
./target/release/host 1111
```
```
secret x = 1111 (never appears in the proof)
--- what the verifier learns ---
  x * x = 1234321
  that the prover knows an x giving that value
  METHOD_ID = [608878370, 3877817843, ...]
--- what the verifier does not learn ---
  x itself

cycles  : 32768
journal : 8 bytes
```

## The whole guest

```rust
use risc0_zkvm::guest::env;

fn main() {
    let x: u64 = env::read();   // SECRET — received from the host
    let target = x * x;         // ordinary Rust
    env::commit(&target);       // PUBLIC — written to the journal
    assert!(x > 1);             // a panic means no proof is produced
}
```

A zkVM is a RISC-V CPU whose execution is proved instruction by instruction. There are
no circuits or constraints to write — compile ordinary Rust and a proof of its correct
execution comes with it. Two programs are involved: the **guest** runs inside the zkVM,
the **host** feeds it input, proves, and verifies.

Four things matter.

**`env::read()`** takes input the host wrote. It is invisible to the verifier unless
committed. **Secrecy is the default.**

**`env::commit()`** writes to the journal. This is all the verifier sees, so *designing
the journal is designing the claim* — the most consequential decision in any zkVM
program.

**`IMAGE_ID`** is a hash of the guest binary, and verification is always against it:

```rust
receipt.verify(METHOD_ID)
```

Without it you only learn that *some* program terminated normally. Change one character
of the guest and the ID changes, so the verifier can confirm which code ran.

**A panic means no proof.** Running `./target/release/host 1` gives:

```
proving failed: Guest panicked: x must be greater than 1
```

So *terminating normally is itself part of the claim*, and a false claim cannot be
proved. This is the foundation of the main project's soundness: type-checking failure
panics nanoda, so a proof of a false theorem cannot exist.

## The receipt

```
receipt = journal (public values) + seal (the proof)
```

If `receipt.verify(IMAGE_ID)` passes, the verifier may believe that **the program with
that image ID produced this journal and terminated normally**. The inputs remain
unknown.

`./target/release/host <x> <rounds>` scales the workload via the second argument.

## Mapping to the main project

| Here | `zkvm/` |
| --- | --- |
| `x: u64` | the Lean export file (the proof term) |
| `x * x` | kernel type-checking by nanoda |
| committing `target` | committing the statement digest and axioms used |
| `assert!` | type-checking failure |

The structure is identical; only the guest's contents differ.
