//! zkVM guest: Lean 4 の export file (NDJSON) を kernel 型検査する。
//!
//! 入力 (private witness): export file のバイト列 + nanoda の設定 JSON
//! journal (public output): 入力のダイジェスト、検査した宣言数、
//!                          allowlist 外だったため skip された axiom 名
//!
//! Phase 1 の目的はサイクル数の計測なので、public/private の分割
//! (statement のコミットメント) はまだ入れていない。

mod io;

use risc0_zkvm::guest::env;
use risc0_zkvm::sha::{Impl, Sha256};

fn main() {
    let ndjson = io::read_bytes();

    let start = env::cycle_count();

    // SHA-256 は risc0 の accelerator 回路で処理されるので、
    // ここは NDJSON パースに比べて桁違いに安い。
    let digest = *Impl::hash_bytes(&ndjson);

    let cycles_hash = env::cycle_count();

    // ポリシーは guest 内に固定する（stmt::policy を参照）。
    let (export_file, skipped_axioms) =
        nanoda_lib::parser::parse_export_file_binary(&ndjson, stmt::policy::config())
            .expect("failed to parse export file");

    let cycles_parse = env::cycle_count();

    let num_declars = export_file.declars.len();

    // 型検査本体。失敗すれば panic し、proof は生成されない。
    export_file.check_all_declars();

    let cycles_check = env::cycle_count();

    env::log(&format!(
        "cycles: hash={} parse={} check={} total={} | declars={}",
        cycles_hash - start,
        cycles_parse - cycles_hash,
        cycles_check - cycles_parse,
        cycles_check - start,
        num_declars,
    ));

    env::commit(&(
        digest,
        num_declars as u64,
        skipped_axioms,
    ));
}
