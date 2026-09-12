//! Passing byte strings in and out.
//!
//! RISC Zero's serde encodes `Vec<u8>` as one u32 word per byte, so handing over a
//! few hundred KB of export file costs tens of millions of cycles on its own.
//! A length plus raw words is a quarter of the data and skips serde entirely.

use risc0_zkvm::guest::env;

pub fn read_bytes() -> Vec<u8> {
    let len: u32 = env::read();
    let mut words = vec![0u32; len.div_ceil(4) as usize];
    env::read_slice(&mut words);
    let mut bytes: Vec<u8> = bytemuck::cast_slice(&words).to_vec();
    bytes.truncate(len as usize);
    bytes
}
