//! Compress a succinct receipt to Groth16, for on-chain verification.
//!
//!   compress <succinct.bin> <groth16.bin>
//!
//! `compose --groth16` runs everything from STARK generation; this **only re-wraps an
//! existing receipt**. They are separate because STARKs are fast on GPU while the
//! Groth16 step (sppark's BN254 MSM) fails on some of them — Turing (sm_75) aborts
//! with `illegal memory access`. This way the STARK can run on GPU and the wrap on
//! CPU.
//!
//! Compression leaves the journal unchanged. Only the seal changes, from a 224 KB
//! STARK to a few hundred bytes of SNARK.

use risc0_zkvm::{default_prover, ProverOpts, Receipt};
use std::time::Instant;

fn main() {
    let mut a = std::env::args().skip(1);
    let (src, dst) = match (a.next(), a.next()) {
        (Some(s), Some(d)) => (s, d),
        _ => {
            eprintln!("usage: compress <succinct.bin> <groth16.bin>");
            std::process::exit(2);
        }
    };

    let bytes = std::fs::read(&src).expect("cannot read receipt");
    let receipt: Receipt = bincode::deserialize(&bytes).expect("cannot decode receipt");
    let journal = receipt.journal.bytes.clone();

    let t = Instant::now();
    let compressed = default_prover()
        .compress(&ProverOpts::groth16(), &receipt)
        .expect("groth16 compression failed");
    let elapsed = t.elapsed().as_secs_f64();

    assert_eq!(compressed.journal.bytes, journal, "the journal must not change");

    let out = bincode::serialize(&compressed).expect("serialize");
    std::fs::write(&dst, &out).expect("cannot write");

    println!("{src} ({} bytes)", bytes.len());
    println!("  -> {dst} ({} bytes)  {elapsed:.1}s", out.len());
    println!("journal unchanged: {} bytes", journal.len());
}
