//! Measure how many of the prelude's declarations are actually read while checking a theorem.
//!
//!   touched <prelude.bin> <full.bin>
//!
//! This bounds what Merkle-based lazy loading — passing only touched declarations in
//! the witness — can achieve.

use nanoda_lib::parser::parse_export_file_binary;

fn main() {
    let prelude_path = std::env::args().nth(1).expect("usage: touched <prelude.bin> <full.bin>");
    let full_path = std::env::args().nth(2).expect("usage: touched <prelude.bin> <full.bin>");
    let prelude = std::fs::read(&prelude_path).unwrap();
    let full = std::fs::read(&full_path).unwrap();

    // Read the prelude alone and count declarations
    let (p, _) = parse_export_file_binary(&prelude, stmt::policy::config()).unwrap();
    let n_prelude = p.declars.len();
    drop(p);

    let (ef, _) = parse_export_file_binary(&full, stmt::policy::config()).unwrap();
    let n_all = ef.declars.len();

    // --- the Stage 2 equivalent: skip the prelude, check only the theorem side ---
    nanoda_lib::touch_trace::reset();
    ef.check_declars_skipping(n_prelude);
    let (touched, unfolded) = nanoda_lib::touch_trace::counts();

    let in_prelude = nanoda_lib::touch_trace::touched_indices()
        .into_iter()
        .filter(|i| *i < n_prelude)
        .count();

    println!("declarations in prelude     : {n_prelude}");
    println!("declarations overall        : {n_all}  (theorem side {})", n_all - n_prelude);
    println!();
    println!("declarations read           : {touched}");
    println!("  of which in the prelude   : {in_prelude} / {n_prelude}  ({:.0}%)",
             100.0 * in_prelude as f64 / n_prelude as f64);
    println!("declarations unfolded       : {unfolded}");
    println!();
    let nodes = nanoda_lib::touch_trace::node_count();
    let total_nodes = std::fs::read(&full_path)
        .map(|b| {
            let mut n = 0usize;
            let mut pos = 0usize;
            while pos + 4 <= b.len() {
                let len = u32::from_le_bytes(b[pos..pos + 4].try_into().unwrap()) as usize;
                pos += 4 + len;
                n += 1;
            }
            n
        })
        .unwrap_or(0);
    println!("term nodes read             : {nodes} / {total_nodes}  ({:.0}%)",
             100.0 * nodes as f64 / total_nodes as f64);
    let depth = (total_nodes as f64).log2().ceil() as usize;
    println!("  tree depth log2({total_nodes})    : {depth}");
    println!("  naive path verification   : {} hashes", nodes * depth);
    println!("  rebuilding the whole tree : {total_nodes} hashes");
    println!();
    println!("→ upper bound: lazy loading can cut the prelude witness to {:.0}%",
             100.0 * in_prelude as f64 / n_prelude as f64);
}
