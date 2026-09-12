//! Stage 1: 環境（prelude）のうち要求された依存閉包を型検査し、
//! その内容と検査済み集合にコミットする。
//!
//! 同じ receipt は、その検査済み集合だけで足りる定理なら使い回せる。
//!
//! journal (public):
//!   - prelude のバイト列の SHA-256
//!   - 全binary recordのMerkle rootとrecord数
//!   - public宣言名集合のMerkle rootと名前数
//!   - prelude の総宣言数
//!   - 検査済み宣言名の集合
//!   - allowlist 外だったため skip された axiom 名

mod io;

use risc0_zkvm::guest::env;
use risc0_zkvm::sha::{Impl, Sha256};

fn main() {
    let requested: Vec<u32> = env::read();
    let prelude = io::read_bytes();

    let start = env::cycle_count();

    // この receipt が「どの prelude について」のものかを固定する。
    let digest = *Impl::hash_bytes(&prelude);

    // ポリシーは guest 内に固定する。witness から受け取ると prover が
    // unsafe_permit_all_axioms を立てられてしまう（stmt::policy を参照）。
    let (export_file, mut skipped_axioms) =
        nanoda_lib::parser::parse_export_file_binary(&prelude, stmt::policy::config())
            .expect("failed to parse prelude");
    skipped_axioms.sort();

    let num_declars = export_file.declars.len() as u64;
    let roots: Vec<usize> = requested
        .into_iter()
        .map(|idx| usize::try_from(idx).expect("declaration index does not fit usize"))
        .collect();
    let checked = export_file.check_declar_closure(&roots, num_declars as usize);
    let mut checked_names: Vec<String> = export_file.with_ctx(|ctx| {
        checked
            .iter()
            .map(|idx| {
                let (_, declar) = export_file.declars.get_index(*idx).expect("checked declaration disappeared");
                stmt::name_to_string(ctx, declar.info().name)
            })
            .collect()
    });
    checked_names.sort();
    let mut all_declaration_names: Vec<String> = export_file.with_ctx(|ctx| {
        export_file
            .declars
            .values()
            .map(|declar| stmt::name_to_string(ctx, declar.info().name))
            .collect()
    });
    all_declaration_names.extend(skipped_axioms.iter().cloned());
    all_declaration_names.sort();
    let (names_root, names_count) = stmt::merkle::names_root(&all_declaration_names)
        .expect("public declaration names are not unique");
    let (record_root, record_count) = stmt::merkle::root(&prelude).expect("invalid prelude record stream");

    env::log(&format!(
        "envcheck: cycles={} checked={}/{}",
        env::cycle_count() - start,
        checked_names.len(),
        num_declars
    ));

    env::commit(&(
        digest,
        record_root,
        record_count,
        names_root,
        names_count,
        num_declars,
        checked_names,
        skipped_axioms,
    ));
}
