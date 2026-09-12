//! For verifiers: type-check the public prelude yourself.
//!
//!   checkprelude <public.bin>
//!
//! `verify` establishes that the theorem side type-checks; whether the prelude is a
//! well-typed environment is accepted as an assumption under `assumed` mode. This
//! command discharges that assumption.
//!
//! **No Lean installation is needed** — just nanoda, this Rust binary. It does not use
//! the zkVM either, so it is fast. Done once, it covers every proof against that
//! prelude.
//!
//! Stage 1 (`certified` mode) does the same thing inside the zkVM, orders of magnitude
//! more expensively. **If the verifier can run anything at all, use this instead.**
//! Stage 1 is for verifiers that can run nothing, such as a smart contract.

use nanoda_lib::parser::parse_export_file_binary;
use risc0_zkvm::sha::{Impl, Sha256};

#[path = "../audit.rs"]
mod audit;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let emit = args.iter().any(|s| s == "--emit-axiom-fingerprints");
    // What a single declaration means here, for comparing two environments by hand.
    let one = args.iter().position(|s| s == "--fingerprint").and_then(|i| args.get(i + 1)).cloned();
    let path = args.iter().find(|s| !s.starts_with("--")).cloned().unwrap_or_else(|| {
        eprintln!("usage: checkprelude <public.bin>");
        std::process::exit(2);
    });
    let bytes = std::fs::read(&path).unwrap_or_else(|e| {
        eprintln!("cannot read: {e}");
        std::process::exit(1);
    });

    println!("{path}  ({} bytes)", bytes.len());
    println!("  SHA-256     : {}", *Impl::hash_bytes(&bytes));
    match stmt::merkle::root(&bytes) {
        Ok((root, count)) => {
            println!("  Merkle root : {}", stmt::hex(&root));
            println!("  records     : {count}");
        }
        Err(e) => {
            eprintln!("  cannot compute the Merkle root: {e}");
            std::process::exit(1);
        }
    }

    let t = std::time::Instant::now();
    let (export_file, skipped) = parse_export_file_binary(&bytes, stmt::policy::config())
        .unwrap_or_else(|e| {
            eprintln!("\n❌ parsing failed: {e}");
            std::process::exit(1);
        });
    let n = export_file.declars.len();

    // Type-check. A failure panics.
    export_file.check_all_declars();
    println!("\n✅ all {n} declarations type-check ({:.2}s)", t.elapsed().as_secs_f64());

    let extra: Vec<&String> =
        skipped.iter().filter(|a| !stmt::policy::PERMITTED_AXIOMS.contains(&a.as_str())).collect();
    if extra.is_empty() {
        println!("✅ no axioms outside the allowlist");
    } else {
        println!("⚠️  axioms outside the allowlist are present: {extra:?}");
        println!("   (they are excluded from checking, so declarations using them will fail)");
    }

    // Audit the allowlisted axioms themselves.
    //
    // The parser admits an axiom whose **name** is on the allowlist without looking at
    // its type, so "type-checks + no axioms outside the allowlist" does not yet pin an
    // axiom system: a declaration named `propext` with type `∀ p : Prop, p` passes both
    // and proves everything. Compare the full fingerprint against the pinned value.
    if emit {
        println!("\n   pinned fingerprints for the allowlist (maintainers):");
        for name in stmt::policy::PERMITTED_AXIOMS {
            match audit::axiom_fingerprint(&export_file, name) {
                Some(fp) => println!("     (\"{name}\", Some({:?})),", fp),
                None => println!("     (\"{name}\", None),  // not in this prelude"),
            }
        }
    }
    if let Some(name) = &one {
        match export_file.with_ctx(|ctx| {
            export_file
                .declars
                .values()
                .find(|d| stmt::name_to_string(ctx, d.info().name) == *name)
                .map(|d| audit::declar_fingerprint(ctx, d))
        }) {
            Some(fp) => println!("\n   {name} : {}", stmt::hex(&fp)),
            None => {
                eprintln!("\n❌ no declaration named {name}");
                std::process::exit(1);
            }
        }
    }
    let mut audited = Vec::new();
    let mut bad = Vec::new();
    for name in stmt::policy::PERMITTED_AXIOMS {
        if audit::count_declars_named(&export_file, name) == 0 {
            continue;
        }
        if audit::count_declars_named(&export_file, name) > 1 {
            bad.push(format!("{name}: the name resolves to more than one declaration"));
            continue;
        }
        match (audit::axiom_fingerprint(&export_file, name), audit::pinned(name)) {
            (Some(actual), Some(want)) if actual == want => audited.push(name),
            (Some(actual), Some(want)) => bad.push(format!(
                "{name}: not the standard axiom\n     expected {}\n     actual   {}",
                stmt::hex(&want),
                stmt::hex(&actual)
            )),
            (Some(_), None) => {
                bad.push(format!("{name}: on the allowlist, but no pinned fingerprint to audit against"))
            }
            (None, _) => bad.push(format!("{name}: on the allowlist, but not declared as an axiom")),
        }
    }
    if !bad.is_empty() {
        for b in &bad {
            eprintln!("\n❌ axiom audit failed: {b}");
        }
        eprintln!("\n   An allowlisted name with the wrong type admits any proposition.");
        std::process::exit(1);
    }
    if audited.is_empty() {
        println!("✅ axiom audit: this prelude declares none of the allowlisted axioms");
    } else {
        println!("✅ axiom audit: {audited:?} match the pinned type, name and universe parameters");
    }
    println!(
        "   allowlisted but absent here: {:?}",
        stmt::policy::PERMITTED_AXIOMS
            .iter()
            .filter(|n| !audited.contains(n))
            .collect::<Vec<_>>()
    );
    println!();
    println!("Proofs against this prelude reconcile with the SHA-256 and Merkle root above");
    println!("during `verify`. Checking it once is enough.");
}
