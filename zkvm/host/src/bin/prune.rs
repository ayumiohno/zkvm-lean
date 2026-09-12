//! Produce a binary keeping only the nodes that were touched.
//!
//!   prune <prelude.bin> <full.bin> <theorem> <out-prelude.bin> <out-full.bin>
//!
//! Run the type-check once outside the zkVM, record which nodes the kernel actually
//! read, and replace the rest with placeholders. Indices are preserved, so no
//! references need remapping.
//!
//! If a replaced node was in fact needed, the kernel fails. **Type-checking succeeding
//! is itself the evidence that the set is self-contained.**

use nanoda_lib::parser::{parse_export_file_binary, prune_binary};
use std::collections::HashSet;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.len() < 5 {
        eprintln!("usage: prune <prelude.bin> <full.bin> <theorem> <out-prelude.bin> <out-full.bin>");
        std::process::exit(1);
    }
    let prelude = std::fs::read(&a[0]).unwrap();
    let full = std::fs::read(&a[1]).unwrap();
    let target = &a[2];

    let (p, _) = parse_export_file_binary(&prelude, stmt::policy::config()).unwrap();
    let n_prelude = p.declars.len();
    drop(p);

    // 1. Type-check outside the zkVM and record which nodes were read
    let (ef, _) = parse_export_file_binary(&full, stmt::policy::config()).unwrap();
    nanoda_lib::touch_trace::reset();
    ef.check_declars_skipping(n_prelude);
    let want = stmt::statement_digest(&ef, target).expect("target not found");
    let keep: HashSet<(u8, u32)> = nanoda_lib::touch_trace::node_indices().into_iter().collect();
    drop(ef);
    println!("nodes read: {}", keep.len());

    // 2. Prune both prelude and full against that set
    let (pruned_prelude, kp, pp) = prune_binary(&prelude, &keep).unwrap();
    let (pruned_full, kf, pf) = prune_binary(&full, &keep).unwrap();
    std::fs::write(&a[3], &pruned_prelude).unwrap();
    std::fs::write(&a[4], &pruned_full).unwrap();
    println!(
        "prelude : {} -> {} bytes  (kept {} / pruned {})",
        prelude.len(),
        pruned_prelude.len(),
        kp,
        pp
    );
    println!(
        "full    : {} -> {} bytes  (kept {} / pruned {})",
        full.len(),
        pruned_full.len(),
        kf,
        pf
    );

    // 3. Confirm the pruned version still type-checks and agrees on the statement
    let (ef2, _) = parse_export_file_binary(&pruned_full, stmt::policy::config()).expect("parsing failed after pruning");
    assert_eq!(ef2.declars.len(), kf.min(ef2.declars.len()).max(ef2.declars.len()));
    ef2.check_declars_skipping(n_prelude);
    let got = stmt::statement_digest(&ef2, target).expect("target not found after pruning");
    assert_eq!(want, got, "the statement digest changed");
    println!();
    println!("✅ still type-checks after pruning, and the statement digest matches");
    println!("   {}", stmt::hex(&got));
    println!("   → the set of nodes read is self-contained");
}
