//! Attribute `check` cycles by counting kernel operations.
//!
//!   checkprofile <prelude.bin> <full.bin> <theorem>
//!
//! Cycles can only be measured in the zkVM, but **operation counts are identical on
//! the host**. Counts times per-operation cost is the only breakdown method that has
//! worked reliably here.

use nanoda_lib::parser::parse_export_file_binary;
use nanoda_lib::touch_trace::ops;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.len() < 3 {
        eprintln!("usage: checkprofile <prelude.bin> <full.bin> <theorem>");
        std::process::exit(2);
    }
    let prelude = std::fs::read(&a[0]).expect("prelude");
    let full = std::fs::read(&a[1]).expect("full");
    assert!(full.starts_with(&prelude), "full must contain prelude as a prefix");

    let n_prelude = parse_export_file_binary(&prelude, stmt::policy::config())
        .expect("prelude parse")
        .0
        .declars
        .len();
    let (ef, _) = parse_export_file_binary(&full, stmt::policy::config()).expect("full parse");
    let n_all = ef.declars.len();

    // The Stage 2 equivalent: skip the prelude and check only the theorem side.
    ops::reset();
    let t = std::time::Instant::now();
    ef.check_declars_skipping(n_prelude);
    let elapsed = t.elapsed();

    println!("{}  (checked {} of {} declarations, native {:.2}s)",
             a[2], n_all, n_all - n_prelude, elapsed.as_secs_f64());
    let counts = ops::read();
    let total: u64 = counts.iter().map(|(_, v)| *v).sum();
    for (name, v) in &counts {
        if *v > 0 {
            println!("  {name:<12} {v:>12}  ({:>5.1}%)", 100.0 * *v as f64 / total.max(1) as f64);
        }
    }
    println!("  {:<12} {total:>12}", "total");

    // See what is actually being checked. Hundreds of declarations for a single
    // theorem usually means something that belongs on the public side ended up on the
    // private side.
    let names: Vec<String> = ef.with_ctx(|ctx| {
        (n_prelude..n_all)
            .map(|i| {
                let (_, d) = ef.declars.get_index(i).expect("declar");
                stmt::name_to_string(ctx, d.info().name)
            })
            .collect()
    });
    let mut by_prefix: std::collections::BTreeMap<String, usize> = Default::default();
    for n in &names {
        let key = n.split('.').take(3).collect::<Vec<_>>().join(".");
        *by_prefix.entry(key).or_default() += 1;
    }
    let mut v: Vec<_> = by_prefix.into_iter().collect();
    v.sort_by_key(|(_, c)| std::cmp::Reverse(*c));
    // --names prints the checked names verbatim, for feeding back to lean4export as
    // public roots.
    if std::env::args().any(|a| a == "--names") {
        for n in &names {
            println!("{n}");
        }
        return;
    }

    println!("\n  namespaces being checked (top 12):");
    for (k, c) in v.iter().take(12) {
        println!("    {c:>5}  {k}");
    }
}
