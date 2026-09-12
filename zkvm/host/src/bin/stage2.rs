//! Run Stage 2 alone and count cycles.
//!
//!   stage2 <prelude.bin> <full.bin> <theorem>
//!
//! Stage 1's journal stands in as an unresolved assumption, but Stage 2 itself runs
//! the same Merkle multiproof verification, index compaction and type-checking as in
//! production.

use methods::{THMCHECK_ELF, THMCHECK_ID};
use nanoda_lib::parser::{parse_export_file_binary, select_sparse_prelude_records};
use risc0_zkvm::sha::{Digest, Digestible, Impl, Sha256};
use risc0_zkvm::{default_executor, ExecutorEnv, MaybePruned, ReceiptClaim};

fn write_bytes(b: &mut risc0_zkvm::ExecutorEnvBuilder, data: &[u8]) {
    b.write(&(data.len() as u32)).unwrap();
    let mut padded = data.to_vec();
    padded.resize(data.len().div_ceil(4) * 4, 0);
    b.write_slice(bytemuck::cast_slice::<u8, u32>(&padded));
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let prelude = std::fs::read(&a[0]).unwrap();
    let full = std::fs::read(&a[1]).unwrap();
    let target = a[2].clone();
    assert!(
        full.starts_with(&prelude),
        "full must contain prelude as a byte prefix"
    );
    let suffix = full[prelude.len()..].to_vec();

    let (p, mut prelude_skipped) = parse_export_file_binary(&prelude, stmt::policy::config()).unwrap();
    prelude_skipped.sort();
    let n_prelude = p.declars.len() as u64;
    let mut checked: Vec<String> = p.with_ctx(|ctx| {
        p.declars
            .values()
            .map(|declar| stmt::name_to_string(ctx, declar.info().name))
            .collect()
    });
    checked.sort();
    let mut public_names = checked.clone();
    public_names.extend(prelude_skipped.iter().cloned());
    public_names.sort();
    let name_indices = (0..p.declars.len())
        .map(|idx| p.declaration_name_index(idx).unwrap())
        .collect();
    drop(p);
    let selected = select_sparse_prelude_records(&prelude, &suffix, &name_indices).unwrap();
    let selected_indices: Vec<u32> = selected.iter().map(|(idx, _)| *idx).collect();
    let mut sparse = stmt::merkle::prove(&prelude, &selected_indices).unwrap();
    // Build name-tree non-membership proofs for the names the suffix defines.
    let suffix_names: Vec<String> = {
        let (all, _) = parse_export_file_binary(&full, stmt::policy::config()).unwrap();
        all.with_ctx(|ctx| {
            all.declars
                .values()
                .skip(n_prelude as usize)
                .map(|declar| stmt::name_to_string(ctx, declar.info().name))
                .collect()
        })
    };
    sparse.names = stmt::merkle::prove_names(&public_names, &suffix_names).unwrap();
    let sparse_blob = stmt::merkle::encode(&sparse);
    let use_sparse = sparse_blob.len().saturating_mul(3) < prelude.len();

    // Assemble Stage 1's journal; nothing was actually proved, so it stays an assumption
    let digest: Digest = *Impl::hash_bytes(&prelude);
    let env_journal: Vec<u8> = {
        let (record_root, record_count) = stmt::merkle::root(&prelude).unwrap();
        let (names_root, names_count) = stmt::merkle::names_root(&public_names).unwrap();
        let words = risc0_zkvm::serde::to_vec(&(
            digest,
            record_root,
            record_count,
            names_root,
            names_count,
            n_prelude,
            checked,
            prelude_skipped,
        ))
        .unwrap();
        bytemuck::cast_slice::<u32, u8>(&words).to_vec()
    };

    let mut b = ExecutorEnv::builder();
    b.write(&target).unwrap();
    b.write(&use_sparse).unwrap();
    write_bytes(&mut b, if use_sparse { &sparse_blob } else { &prelude });
    write_bytes(&mut b, &suffix);
    write_bytes(&mut b, &env_journal);
    b.add_assumption(ReceiptClaim::ok(
        methods::ENVCHECK_ID,
        MaybePruned::Pruned(env_journal.digest()),
    ));

    let s = default_executor()
        .execute(b.build().unwrap(), THMCHECK_ELF)
        .expect("stage 2 failed");
    println!(
        "prelude={} bytes sparse={} bytes suffix={} bytes  ->  Stage 2 = {} cycles  (image {:?})",
        prelude.len(),
        sparse_blob.len(),
        suffix.len(),
        s.cycles(),
        &THMCHECK_ID[..1]
    );
}
