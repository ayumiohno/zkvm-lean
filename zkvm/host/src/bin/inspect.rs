//! Show a receipt's contents, for explanation and debugging.
//!
//!   inspect <receipt.bin>
//!
//! A receipt is a journal (the public data) plus a seal (the cryptographic proof).
//! The seal is not meant to be read; it is what `verify` checks mathematically.

use risc0_zkvm::sha::Digestible;
use risc0_zkvm::{InnerReceipt, Receipt};

fn main() {
    let path = std::env::args().nth(1).expect("usage: inspect <receipt.bin>");
    let bytes = std::fs::read(&path).expect("cannot read");
    let receipt: Receipt = bincode::deserialize(&bytes).expect("cannot decode");

    println!("whole file             : {} bytes", bytes.len());
    println!();
    println!("1. journal (public)    : {} bytes", receipt.journal.bytes.len());
    println!("   digest              : {}", receipt.journal.digest());
    println!();
    let kind = match &receipt.inner {
        InnerReceipt::Composite(c) => format!("Composite ({} per-segment STARKs)", c.segments.len()),
        InnerReceipt::Succinct(_) => "Succinct (recursively compressed to one STARK)".to_string(),
        InnerReceipt::Groth16(_) => "Groth16 (a SNARK, for on-chain use)".to_string(),
        InnerReceipt::Fake(_) => "Fake (development only; no cryptographic guarantee)".to_string(),
        _ => "unknown".to_string(),
    };
    println!("2. seal (the proof)    : {} bytes", bytes.len() - receipt.journal.bytes.len());
    println!("   kind                : {kind}");
    println!();
    // If this is a thmcheck journal, decode it: this is all a verifier can see.
    type ThmJournal =
        (bool, [u32; 8], risc0_zkvm::sha::Digest, [u8; 32], [u8; 32], String, [u8; 32], u64, Vec<String>);
    if let Ok((certified, env_id, prelude_digest, record_root, names_root, name, statement, n, skipped)) =
        receipt.journal.decode::<ThmJournal>()
    {
        println!("   journal contents (everything the verifier can see)");
        println!("     trust mode        : {}", if certified { "certified" } else { "assumed" });
        if certified {
            println!("     envcheck image id: {}", risc0_zkvm::sha::Digest::from(env_id));
        }
        println!("     prelude SHA-256   : {prelude_digest}");
        println!("     record root       : {}", stmt::hex(&record_root));
        println!("     name root         : {}", stmt::hex(&names_root));
        println!("     theorem name      : {name}");
        println!("     statement         : {}", stmt::hex(&statement));
        println!("     declarations      : {n}");
        println!("     axioms skipped    : {skipped:?}");
        println!();
    }

    match receipt.claim() {
        Ok(claim) => {
            println!("3. claim (what is asserted)");
            if let Ok(c) = claim.as_value() {
                println!("   image id (which program)  : {}", c.pre.digest());
                println!("   exit code                   : {:?}", c.exit_code);
            }
            println!("   claim digest              : {}", claim.digest());
        }
        Err(e) => println!("3. cannot extract the claim: {e}"),
    }
}
