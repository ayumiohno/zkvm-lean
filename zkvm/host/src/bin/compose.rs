//! Driver for the two-stage configuration.
//!
//! ```text
//! compose <prelude.bin> <full.bin> <theorem> [options]
//!
//!   --prove                actually generate a proof (default: executor only)
//!   --env-receipt <path>   read a Stage 1 receipt; create and save one if absent
//!   --out <path>           write the Stage 2 receipt (the only artefact a verifier gets)
//!   --groth16              compress Stage 2 to Groth16, for on-chain verification
//! ```
//!
//! `full.bin` must contain `prelude.bin` as a byte prefix, which is what
//! `lean4export Mod -- <prelude constants> <theorem>` produces in that order.
//!
//! **`--env-receipt` is where composition pays off.** If a Stage 1 receipt's checked
//! set also covers the next theorem's dependencies, that receipt is reused; otherwise
//! a new Stage 1 is built for the set actually needed.

use methods::{ENVCHECK_ELF, ENVCHECK_ID, THMCHECK_ELF, THMCHECK_ID};
use nanoda_lib::parser::{parse_export_file_binary, select_sparse_prelude_records};
use risc0_zkvm::sha::{Digest, Digestible, Impl, Sha256};
use risc0_zkvm::{
    default_executor, default_prover, ExecutorEnv, ExecutorEnvBuilder, MaybePruned, ProverOpts,
    Receipt, ReceiptClaim,
};
use std::time::Instant;

const USAGE: &str = "usage: compose <prelude.bin> <full.bin> <theorem> \
                     [--prove] [--env-receipt <path>] [--out <path>] [--groth16] [--full] \
                     [--assume-prelude]";

struct Args {
    prelude_path: String,
    full_path: String,
    target_name: String,
    prove: bool,
    env_receipt: Option<String>,
    out: Option<String>,
    groth16: bool,
    /// Pass the full prelude instead of a sparse witness, for A/B comparison.
    force_full: bool,
    /// Use sparse regardless of the threshold, for A/B comparison.
    force_sparse: bool,
    /// Skip Stage 1 and trust the prelude. The only option at Mathlib scale.
    assume_prelude: bool,
}

fn parse_args() -> Args {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let positional: Vec<&String> = a.iter().filter(|s| !s.starts_with("--")).collect();
    let flag = |name: &str| a.iter().any(|s| s == name);
    let value = |name: &str| {
        a.iter().position(|s| s == name).map(|i| {
            a.get(i + 1)
                .unwrap_or_else(|| panic!("{name} requires a value\n{USAGE}"))
                .clone()
        })
    };
    if positional.len() < 3 {
        eprintln!("{USAGE}");
        std::process::exit(2);
    }
    Args {
        prelude_path: positional[0].clone(),
        full_path: positional[1].clone(),
        target_name: positional[2].clone(),
        prove: flag("--prove"),
        env_receipt: value("--env-receipt"),
        out: value("--out"),
        groth16: flag("--groth16"),
        force_full: flag("--full"),
        force_sparse: flag("--sparse"),
        assume_prelude: flag("--assume-prelude"),
    }
}

fn write_bytes(builder: &mut ExecutorEnvBuilder, data: &[u8]) {
    builder.write(&(data.len() as u32)).unwrap();
    let mut padded = data.to_vec();
    padded.resize(data.len().div_ceil(4) * 4, 0);
    builder.write_slice(bytemuck::cast_slice::<u8, u32>(&padded));
}

/// Load a saved Stage 1 receipt and confirm it belongs to this prelude.
///
/// Besides verifying the receipt itself, **the journal's prelude digest is compared**.
/// Skipping that would allow recycling a receipt from a different environment.
fn load_env_receipt(path: &str, prelude: &[u8], required: &[String]) -> Option<Receipt> {
    let bytes = std::fs::read(path).ok()?;
    let receipt: Receipt = bincode::deserialize(&bytes).expect("cannot decode the env receipt");
    receipt.verify(ENVCHECK_ID).ok()?;

    let (digest, _root, _records, _names_root, _names_n, _n, checked, _skipped):
        (Digest, [u8; 32], u32, [u8; 32], u32, u64, Vec<String>, Vec<String>) =
        receipt.journal.decode().ok()?;
    let want = *Impl::hash_bytes(prelude);
    assert_eq!(digest, want, "the stored env receipt belongs to a different prelude");
    if !required
        .iter()
        .all(|name| checked.binary_search(name).is_ok())
    {
        return None;
    }
    Some(receipt)
}

/// Native execution is only a hint generator: Stage 2 independently records
/// its real lookups and rejects a certificate that omitted any of them.
struct RequiredPrelude {
    num_declars: u64,
    root_indices: Vec<u32>,
    root_names: Vec<String>,
    root_name_indices: std::collections::HashSet<u32>,
    all_names: Vec<String>,
    /// Declaration names the suffix defines, used to build name-tree non-membership proofs.
    suffix_names: Vec<String>,
}

fn required_prelude_roots(prelude: &[u8], full: &[u8]) -> RequiredPrelude {
    let (p, prelude_skipped) = parse_export_file_binary(prelude, stmt::policy::config())
        .expect("cannot parse prelude while selecting dependencies");
    let n_prelude = p.declars.len();
    let mut all_names: Vec<String> = p.with_ctx(|ctx| {
        p.declars
            .values()
            .map(|declar| stmt::name_to_string(ctx, declar.info().name))
            .collect()
    });
    all_names.extend(prelude_skipped);
    all_names.sort();
    drop(p);

    let (all, _) = parse_export_file_binary(full, stmt::policy::config())
        .expect("cannot parse full export while selecting dependencies");
    nanoda_lib::touch_trace::reset();
    all.check_declars_skipping(n_prelude);
    let root_indices: Vec<u32> = nanoda_lib::touch_trace::touched_indices()
        .into_iter()
        .filter(|idx| *idx < n_prelude)
        .map(|idx| u32::try_from(idx).expect("too many declarations for certificate"))
        .collect();
    let mut root_names: Vec<String> = all.with_ctx(|ctx| {
        root_indices
            .iter()
            .map(|idx| {
                let (_, declar) = all.declars.get_index(*idx as usize).expect("root declaration disappeared");
                stmt::name_to_string(ctx, declar.info().name)
            })
            .collect()
    });
    root_names.sort();
    let root_name_indices = root_indices
        .iter()
        .map(|idx| all.declaration_name_index(*idx as usize).expect("root name disappeared"))
        .collect();
    let suffix_names: Vec<String> = all.with_ctx(|ctx| {
        all.declars
            .values()
            .skip(n_prelude)
            .map(|declar| stmt::name_to_string(ctx, declar.info().name))
            .collect()
    });
    RequiredPrelude {
        num_declars: n_prelude as u64,
        root_indices,
        root_names,
        root_name_indices,
        all_names,
        suffix_names,
    }
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let args = parse_args();

    let prelude = std::fs::read(&args.prelude_path).expect("cannot read prelude");
    let full = std::fs::read(&args.full_path).expect("cannot read full export");
    assert!(
        full.starts_with(&prelude),
        "{} must be a byte prefix of {}",
        args.prelude_path,
        args.full_path
    );
    let suffix = full[prelude.len()..].to_vec();
    let required = required_prelude_roots(&prelude, &full);

    println!("prelude : {} ({} bytes)", args.prelude_path, prelude.len());
    println!("suffix  : {} bytes (the theorem-side delta)", suffix.len());
    println!(
        "candidates: {} / {} prelude declarations (Stage 1 re-derives the closure)",
        required.root_indices.len(),
        required.num_declars
    );
    println!();

    // Under --assume-prelude, Stage 1 is not run; only the environment descriptor is
    // assembled.
    //
    // This is not "trusting the host". The descriptor reaches the journal, where the
    // verifier **compares it against values computed from their own prelude**. The
    // only thing trusted is that the prelude type-checks — which, for a standard
    // prelude, every Lean user already relies on.
    let assumed_descriptor: Option<Vec<u8>> = if args.assume_prelude {
        let (root, count) = stmt::merkle::root(&prelude).expect("cannot compute record root");
        let (names_root, names_count) = stmt::merkle::names_root(&required.all_names)
            .expect("public declaration names are not unique");
        let words = risc0_zkvm::serde::to_vec(&(
            *Impl::hash_bytes(&prelude),
            root,
            count,
            names_root,
            names_count,
            required.num_declars,
            Vec::<String>::new(),
            Vec::<String>::new(),
        ))
        .unwrap();
        Some(bytemuck::cast_slice::<u32, u8>(&words).to_vec())
    } else {
        None
    };

    // ---- Stage 1: check the required prelude dependency closure ----
    let cached = args
        .env_receipt
        .as_deref()
        .and_then(|p| load_env_receipt(p, &prelude, &required.root_names));

    let (env_journal, stage1_receipt) = if let Some(d) = assumed_descriptor {
        println!("Stage 1 (envcheck)  skipped (--assume-prelude; the prelude is presupposed)");
        (d, None)
    } else {
        match cached {
        Some(r) => {
            println!(
                "Stage 1 (envcheck)  reused from {} (proving skipped)",
                args.env_receipt.as_deref().unwrap()
            );
            (r.journal.bytes.clone(), Some(r))
        }
        None => {
            let mut b1 = ExecutorEnv::builder();
            b1.write(&required.root_indices).unwrap();
            write_bytes(&mut b1, &prelude);
            let env1 = b1.build().unwrap();

            let t = Instant::now();
            if args.prove {
                let info = default_prover()
                    .prove_with_opts(env1, ENVCHECK_ELF, &ProverOpts::succinct())
                    .expect("stage 1 proving failed");
                info.receipt
                    .verify(ENVCHECK_ID)
                    .expect("stage 1 verification failed");
                println!(
                    "Stage 1 (envcheck)  cycles={:>12}  {:.1}s",
                    info.stats.total_cycles,
                    t.elapsed().as_secs_f64()
                );
                if let Some(p) = args.env_receipt.as_deref() {
                    std::fs::write(p, bincode::serialize(&info.receipt).unwrap())
                        .expect("cannot write env receipt");
                    println!("                    saved to {p} (reused from now on)");
                }
                (info.receipt.journal.bytes.clone(), Some(info.receipt))
            } else {
                let s = default_executor()
                    .execute(env1, ENVCHECK_ELF)
                    .expect("stage 1 failed");
                println!(
                    "Stage 1 (envcheck)  cycles={:>12}  {:.1}s",
                    s.cycles(),
                    t.elapsed().as_secs_f64()
                );
                (s.journal.bytes.clone(), None)
            }
        }
        }
    };

    // Stage 2 receives the required records and a Merkle multiproof, not the whole prelude.
    let selected_records = select_sparse_prelude_records(
        &prelude,
        &suffix,
        &required.root_name_indices,
    )
    .expect("cannot construct sparse prelude");
    let selected_indices: Vec<u32> = selected_records.iter().map(|(idx, _)| *idx).collect();
    let mut sparse = stmt::merkle::prove(&prelude, &selected_indices).expect("cannot build Merkle multiproof");
    assert_eq!(sparse.records, selected_records, "sparse selector and Merkle leaves disagree");
    // Build non-membership proofs for the names the suffix defines, not the name set itself.
    sparse.names = stmt::merkle::prove_names(&required.all_names, &required.suffix_names)
        .expect("cannot build declaration-name non-membership proof");
    {
        let (names_root, names_count) =
            stmt::merkle::names_root(&required.all_names).expect("names root");
        assert!(
            stmt::merkle::verify_names(names_root, names_count, &sparse.names),
            "name proof self-check failed"
        );
        assert!(
            stmt::merkle::names_absent(&sparse.names, names_count, &required.suffix_names),
            "the suffix redefines a public declaration (a declaration of the same name exists)"
        );
        println!(
            "name tree: opened {} / {} leaves (non-membership; {} suffix declarations)",
            sparse.names.leaves.len(),
            names_count,
            required.suffix_names.len()
        );
    }
    let sparse_blob = stmt::merkle::encode(&sparse);
    {
        // Hash invocations during Merkle verification. Cycle counts come from the
        // zkVM, but the counts are identical on the host, which is what makes
        // cycles-per-call derivable here.
        let (root_hash, count) = stmt::merkle::root(&prelude).expect("root");
        stmt::merkle::counters::reset();
        assert!(stmt::merkle::verify(root_hash, count, &sparse), "self-check failed");
        let (leaf, node, pad) = stmt::merkle::counters::read();
        println!(
            "merkle verification hash calls: leaf={leaf} node={node} pad={pad} total={}",
            leaf + node + pad
        );
    }
    // Decide whether to use sparse input.
    //
    // The threshold used to be "witness under a third of the prelude", but optimising
    // Merkle verification (compress, carrying Digest through, dropping the binary
    // search) made authentication so much cheaper that sparse wins even at 57.6%.
    //
    //   environment  witness%   full        sparse
    //   Preimage sm   57.6%     23.18M      21.45M   sparse wins
    //   Mathlib       41.1%     88.56M      62.02M   sparse wins
    //   Preimage lg   25.9%     20.41M       7.73M   sparse wins
    //
    // Only when the witness is nearly as large as the prelude does the parse saving
    // vanish, and only then do we fall back to full.
    let use_sparse = !args.force_full
        && (args.force_sparse || sparse_blob.len().saturating_mul(10) < prelude.len() * 9);
    println!(
        "sparse   : {} / {} records, {} bytes ({:.1}% of prelude)",
        sparse.records.len(),
        stmt::merkle::records(&prelude).unwrap().len(),
        sparse_blob.len(),
        100.0 * sparse_blob.len() as f64 / prelude.len() as f64,
    );
    println!(
        "Stage 2 input mode: {}",
        if use_sparse { "Merkle sparse" } else { "full prelude (small-environment fallback)" }
    );

    // ---- Stage 2: check the theorem, inheriting the Stage 1 receipt ----
    let mut builder = ExecutorEnv::builder();
    builder.write(&args.target_name).unwrap();
    builder.write(&use_sparse).unwrap();
    builder.write(&!args.assume_prelude).unwrap();
    write_bytes(&mut builder, if use_sparse { &sparse_blob } else { &prelude });
    write_bytes(&mut builder, &suffix);
    write_bytes(&mut builder, &env_journal);

    // Register the assumption the guest's `env::verify` demands.
    //
    // Passing the actual Stage 1 receipt resolves it, making the Stage 2 receipt
    // unconditional and verifiable on its own. When only counting cycles, Stage 1 was
    // never proved, so an unresolved assumption stands in and the receipt stays
    // conditional.
    if !args.assume_prelude {
        match stage1_receipt {
            Some(r) => {
                builder.add_assumption(r);
            }
            None => {
                let claim =
                    ReceiptClaim::ok(ENVCHECK_ID, MaybePruned::Pruned(env_journal.digest()));
                builder.add_assumption(claim);
            }
        }
    }
    let env2 = builder.build().unwrap();

    let t = Instant::now();
    let (journal, stage2_cycles) = if args.prove {
        let opts = if args.groth16 {
            ProverOpts::groth16()
        } else {
            ProverOpts::succinct()
        };
        let info = default_prover()
            .prove_with_opts(env2, THMCHECK_ELF, &opts)
            .expect("stage 2 proving failed");
        info.receipt
            .verify(THMCHECK_ID)
            .expect("stage 2 verification failed");
        if let Some(p) = args.out.as_deref() {
            // The receipt is all the verifier gets, and it holds no proof term.
            std::fs::write(
                p,
                bincode::serialize(&info.receipt).expect("serialize receipt"),
            )
            .expect("cannot write receipt");
            println!(
                "wrote the Stage 2 receipt to {p} ({} bytes)",
                std::fs::metadata(p).map(|m| m.len()).unwrap_or(0)
            );
        }
        (info.receipt.journal.bytes.clone(), info.stats.total_cycles)
    } else {
        let s = default_executor()
            .execute(env2, THMCHECK_ELF)
            .expect("stage 2 failed");
        (s.journal.bytes.clone(), s.cycles())
    };
    println!(
        "Stage 2 (thmcheck)  cycles={:>12}  {:.1}s",
        stage2_cycles,
        t.elapsed().as_secs_f64()
    );

    // ---- the journal: everything made public ----
    let (certified, env_id, prelude_dg, record_root, names_root, name, statement, n_declars, skipped): (
        bool,
        [u32; 8],
        Digest,
        [u8; 32],
        [u8; 32],
        String,
        [u8; 32],
        u64,
        Vec<String>,
    ) = risc0_zkvm::serde::from_slice(&journal).expect("bad journal");

    println!();
    println!("=== journal (everything the verifier can see) ===");
    println!(
        "  environment trust mode        : {}",
        if certified { "certified (Stage 1 checked it)" } else { "assumed (the prelude is presupposed)" }
    );
    println!(
        "  inherited envcheck image id   : {}",
        Digest::from(env_id)
    );
    println!("  prelude digest                : {prelude_dg}");
    println!("  record Merkle root            : {}", stmt::hex(&record_root));
    println!("  declaration-name Merkle root  : {}", stmt::hex(&names_root));
    println!("  theorem name                  : {name}");
    println!(
        "  statement digest              : {}",
        stmt::hex(&statement)
    );
    println!("  declarations checked          : {n_declars}");
    println!("  axioms skipped                : {skipped:?}");
    println!();
    println!("=== what the journal does not carry ===");
    println!("  the proof term itself ({} bytes of suffix)", suffix.len());
}
