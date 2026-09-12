//! The driver that runs outside the zkVM (the host).
//! It feeds the guest its input, generates a proof, and verifies it.

use methods::{METHOD_ELF, METHOD_ID};
use risc0_zkvm::{default_prover, ExecutorEnv};

fn main() {
    let secret: u64 = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "1111".into())
        .parse()
        .expect("the argument must be an integer");

    // 1. Prepare the input. Whatever is written reaches the guest and stays SECRET.
    let rounds: u64 = std::env::args().nth(2).unwrap_or_else(|| "0".into()).parse().unwrap();
    let env = ExecutorEnv::builder().write(&(secret, rounds)).unwrap().build().unwrap();

    println!("secret x = {} (never appears in the proof)", secret);

    // 2. Prove: run the guest and build a proof that the run was correct.
    let prove_info = default_prover().prove(env, METHOD_ELF).expect("proving failed");
    let receipt = prove_info.receipt;

    // --- from here on we are the verifier, holding no secret at all ---

    // 3. Verify. METHOD_ID is a hash of the guest binary, pinning which program ran.
    receipt.verify(METHOD_ID).expect("verification failed");

    // 4. Read the public values out of the journal.
    let (target, _acc): (u64, u64) = receipt.journal.decode().unwrap();

    println!("--- what the verifier learns ---");
    println!("  x * x = {}", target);
    println!("  that the prover knows an x giving that value");
    println!("  METHOD_ID = {:?}", METHOD_ID);
    println!("--- what the verifier does not learn ---");
    println!("  x itself");
    println!();
    println!("rounds  : {}", rounds);
    println!("cycles  : {}", prove_info.stats.total_cycles);
    println!("journal : {} bytes", receipt.journal.bytes.len());
}
