//! Build on-chain calldata from a Groth16 receipt.
//!
//!   calldata <groth16.bin>
//!
//! Emits the three arguments `IRiscZeroVerifier.verify(seal, imageId, journalDigest)` takes.
//!
//! The seal is **4 bytes of selector plus a 256-byte Groth16 proof = 260 bytes**. The
//! selector is the first 4 bytes of the verifier parameters' digest, which is how the
//! contract picks the verifying key.
//!
//! The journal itself never goes on chain — only its digest, and **that one value
//! commits to the prelude, the name root, the theorem name, the statement and the
//! axiom set**.

use risc0_zkvm::sha::{Digest, Digestible};
use risc0_zkvm::Receipt;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| {
        eprintln!("usage: calldata <groth16.bin>");
        std::process::exit(2);
    });
    let bytes = std::fs::read(&path).expect("cannot read receipt");
    let receipt: Receipt = bincode::deserialize(&bytes).expect("cannot decode receipt");

    let groth16 = receipt
        .inner
        .groth16()
        .expect("not a Groth16 receipt (wrap it with compress first)");

    // The same layout as risc0-ethereum's encode_seal.
    let selector = &groth16.verifier_parameters.as_bytes()[..4];
    let mut seal = Vec::with_capacity(selector.len() + groth16.seal.len());
    seal.extend_from_slice(selector);
    seal.extend_from_slice(&groth16.seal);

    let image_id: Digest = receipt
        .claim()
        .expect("cannot extract the claim")
        .as_value()
        .expect("the claim is pruned")
        .pre
        .digest();
    let journal_digest = receipt.journal.digest();

    println!("// IRiscZeroVerifier.verify(seal, imageId, journalDigest)");
    println!();
    println!("imageId       = 0x{image_id}");
    println!("journalDigest = 0x{journal_digest}");
    println!("seal ({} bytes, selector 4 + proof {})", seal.len(), groth16.seal.len());
    println!("  0x{}", hex(&seal));
    println!();
    println!("journal ({} bytes; does not need to go on chain)", receipt.journal.bytes.len());
    println!("  0x{}", hex(&receipt.journal.bytes));
}
