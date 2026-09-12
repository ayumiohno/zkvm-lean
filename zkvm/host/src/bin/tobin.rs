//! Convert NDJSON to the binary format. Runs outside the zkVM.
//!
//!   tobin <export.ndjson> <out.bin>
//!
//! The conversion is deterministic, so a verifier running it gets the same bytes and
//! the same hash. The guest consumes this binary and never parses JSON.

use nanoda_lib::parser::ndjson_to_binary;
use std::io::BufReader;

fn main() {
    let src = std::env::args().nth(1).expect("usage: tobin <export.ndjson> <out.bin>");
    let dst = std::env::args().nth(2).expect("usage: tobin <export.ndjson> <out.bin>");
    let f = std::fs::File::open(&src).expect("cannot open input");
    let bin = ndjson_to_binary(BufReader::new(f)).expect("conversion failed");
    let before = std::fs::metadata(&src).unwrap().len();
    std::fs::write(&dst, &bin).expect("cannot write output");
    println!(
        "{src} {before} bytes -> {dst} {} bytes ({:.0}%)",
        bin.len(),
        100.0 * bin.len() as f64 / before as f64
    );
}
