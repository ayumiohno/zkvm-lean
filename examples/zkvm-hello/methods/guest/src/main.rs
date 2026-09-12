//! The program that runs inside the zkVM (the guest).
//!
//! Proves knowledge of an x with x * x = target, without revealing x.
//! It is ordinary Rust: no circuits, no constraints.
//!
//! The second argument, `rounds`, scales the workload for cost measurement.

use risc0_zkvm::guest::env;

fn main() {
    // SECRET, received from the host. Invisible to everyone unless committed.
    let (x, rounds): (u64, u64) = env::read();

    // Just a computation.
    let target = x * x;

    // A loop to add work, for observing how proving cost tracks cycles.
    let mut acc: u64 = x;
    for _ in 0..rounds {
        acc = acc.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    }

    // PUBLIC. Only what is written to the journal is visible to the verifier.
    env::commit(&(target, acc));

    // A panic here means no proof is produced, so terminating normally is itself
    // part of the claim.
    assert!(x > 1, "x must be greater than 1");
}
