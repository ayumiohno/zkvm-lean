//! Stage 2: 環境を引き継いで、定理だけを型検査する。
//!
//! 環境の扱いに 2 つのモードがある。journal にどちらを使ったかを必ず出すので、
//! 検証者は自分のポリシーに合う方だけを受け入れられる。
//!
//! ## `certified` — Stage 1 の receipt を引き継ぐ
//!
//! Stage 1 が prelude の必要部分を型検査し、その証明書を渡す。ここでは定理側を
//! 検査し、実際に参照した prelude 宣言がすべて Stage 1 の検査済み集合に入ることを
//! 確かめる。**prelude を信頼しなくてよい。**
//!
//! ## `assumed` — prelude を信頼する（Stage 1 なし）
//!
//! Merkle root などを **public input として受け取り**、journal にそのまま出す。
//! 検証者は自分の prelude から同じ値を計算して照合する。prelude が型検査を
//! 通ることは **証明せず前提にする**。
//!
//! Mathlib 規模では Stage 1 が計算不可能（1 宣言あたり約 1.09M cycles、Init だけで
//! 58,143 宣言）なので、標準 prelude を使うならこちらになる。「Mathlib は型が付く」は
//! Lean 利用者が全員すでに置いている前提なので、追加の信頼にはならない。
//! 逆に自作の未検証な prelude なら `certified` を使うべき。
//!
//! journal (public):
//!   - 環境の信頼モード（`certified` / `assumed`）
//!   - 引き継いだ Stage 1 の image id（`assumed` ではゼロ）
//!   - prelude の SHA-256、record の Merkle root、public 宣言名の Merkle root
//!   - 定理名、statement の正準ダイジェスト
//!   - 検査した宣言の総数と、skip された axiom 名

mod io;

// ビルド時に確定した Stage 1 の image ID（methods/build.rs が生成）。
// witness から受け取ると、悪意ある Stage 1 の receipt を引き込まれてしまう。
include!("pinned_envcheck_id.rs");

use risc0_zkvm::guest::env;
use risc0_zkvm::sha::{Digest, Impl, Sha256};

fn main() {
    let target_name: String = env::read();
    let sparse_mode: bool = env::read();
    // true なら Stage 1 の receipt を引き継ぐ。false なら prelude を信頼する。
    let certified: bool = env::read();
    let prelude_input = io::read_bytes();
    let suffix = io::read_bytes();
    let env_journal = io::read_bytes();

    let start = env::cycle_count();

    let (
        prelude_digest,
        record_root,
        record_count,
        names_root,
        names_count,
        num_prelude_declars,
        checked_prelude,
        env_skipped,
    ): (Digest, [u8; 32], u32, [u8; 32], u32, u64, Vec<String>, Vec<String>) = if certified {
        // 1. 「envcheck_id のプログラムが env_journal を出して正常終了した receipt が
        //    存在する」ことを仮定として引き込む。ホストがその receipt を用意できなければ、
        //    ここで作った証明は検証を通らない。
        env::verify(Digest::from(PINNED_ENVCHECK_ID), env_journal.as_slice())
            .expect("env receipt missing");
        // 2. その journal が、いま手元にある prelude についてのものであることを確認する。
        //    これを飛ばすと、別の（都合のよい）環境の receipt を流用できてしまう。
        risc0_zkvm::serde::from_slice(&env_journal).expect("bad env journal")
    } else {
        // Stage 1 なし。環境の記述子を public input として受け取り、journal に出す。
        //
        // ここでホストを信頼しているわけではない。これらの値は journal に出るので、
        // 検証者が **自分の prelude から計算した値と照合する**。食い違えば検証が落ちる。
        // 信頼しているのは「その prelude が型検査を通ること」だけ。
        risc0_zkvm::serde::from_slice(&env_journal).expect("bad env descriptor")
    };

    let cycles_verify = env::cycle_count();

    // 3. Stage 1 がコミットした root に対する membership を確認してから、
    //    original index を密な index に付け替える。未選択レコードは生成しない。
    let mut auth_decode = 0u64;
    let mut auth_merkle = 0u64;
    let sparse = if sparse_mode {
        let t0 = env::cycle_count();
        let sparse = stmt::merkle::decode(&prelude_input).expect("bad sparse witness encoding");
        let t1 = env::cycle_count();
        auth_decode = t1 - t0;
        assert!(
            stmt::merkle::verify(record_root, record_count, &sparse),
            "sparse prelude Merkle proof is invalid"
        );
        auth_merkle = env::cycle_count() - t1;
        // 名前木から開いた葉も同じ root に対して検証する。ここでは葉が本物だと
        // 確かめるだけで、「suffix の名前がその間に無い」の判定は parse の後。
        assert!(
            stmt::merkle::verify_names(names_root, names_count, &sparse.names),
            "public declaration-name proof is invalid"
        );
        Some(sparse)
    } else {
        assert_eq!(
            *Impl::hash_bytes(&prelude_input),
            prelude_digest,
            "full prelude does not match Stage 1"
        );
        None
    };
    let cycles_merkle = env::cycle_count();

    // ポリシーは guest 内に固定する（stmt::policy を参照）。
    //
    // sparse では index の密化と DAG の構築を 1 パスで行う。以前は
    // compact_sparse_export で中間のバイト列を作ってから parse し直しており、
    // 同じ record を最大 3 回 deserialize していた。
    let (export_file, mut skipped_axioms, num_compact_prelude_declars) =
        if let Some(sparse) = &sparse {
            nanoda_lib::parser::parse_sparse_export(
                &sparse.records,
                &suffix,
                stmt::policy::config(),
            )
            .expect("failed to parse sparse export")
        } else {
            let mut all = prelude_input;
            all.extend_from_slice(&suffix);
            let (ef, skipped) =
                nanoda_lib::parser::parse_export_file_binary(&all, stmt::policy::config())
                    .expect("failed to parse export file");
            (ef, skipped, num_prelude_declars as usize)
        };
    let cycles_compact = cycles_merkle;
    skipped_axioms.extend(env_skipped.iter().cloned());
    skipped_axioms.sort();
    skipped_axioms.dedup();

    let suffix_names: Vec<String> = export_file.with_ctx(|ctx| {
        export_file
            .declars
            .values()
            .skip(num_compact_prelude_declars)
            .map(|declar| stmt::name_to_string(ctx, declar.info().name))
            .collect()
    });
    // sparse では public 宣言名を集合ごと受け取らず、**suffix が定義する名前が
    // 入る位置の左右の葉だけ**を開いて「そこに無い」ことを示す。集合全体を
    // 渡していた頃はここだけが O(prelude 宣言数) で、Mathlib 規模では
    // 114.5M cycles かかっていた。
    match &sparse {
        Some(sparse) => assert!(
            stmt::merkle::names_absent(&sparse.names, names_count, &suffix_names),
            "private suffix redefines a public declaration"
        ),
        // full 入力では prelude 全体を parse するので、同名の宣言は
        // そこで衝突する。skip された axiom だけ別に見る。
        None => assert!(
            stmt::suffix_avoids_public_names(&env_skipped, &suffix_names),
            "private suffix redefines a public declaration"
        ),
    }

    let cycles_parse = env::cycle_count();

    let num_declars = export_file.declars.len() as u64;
    assert!(
        num_declars >= num_compact_prelude_declars as u64,
        "compact export is shorter than its prelude"
    );
    // Stage 2 が本当に使った prelude 宣言を checker 自身に記録させる。
    // ホストが Stage 1 用に提案した依存集合は信頼せず、ここで照合する。
    nanoda_lib::touch_trace::reset();
    export_file.check_declars_skipping(num_compact_prelude_declars);
    let touched_prelude_indices: Vec<usize> = nanoda_lib::touch_trace::touched_indices()
        .into_iter()
        .filter(|idx| *idx < num_compact_prelude_declars)
        .collect();
    let touched_prelude: Vec<String> = export_file.with_ctx(|ctx| {
        touched_prelude_indices
            .iter()
            .map(|idx| {
                let (_, declar) = export_file.declars.get_index(*idx).expect("touched declaration disappeared");
                stmt::name_to_string(ctx, declar.info().name)
            })
            .collect()
    });
    // `certified` のときだけ、触った prelude 宣言が Stage 1 の検査済み集合に
    // 入っていることを確かめる。`assumed` では prelude 全体を前提にしているので
    // 照合する相手が無い（その前提は journal のモードとして公開される）。
    if certified {
        assert!(
            stmt::name_certificate_covers(&checked_prelude, &touched_prelude),
            "theorem used a prelude declaration not certified by Stage 1"
        );
    }

    let cycles_check = env::cycle_count();

    // 4. 何を証明したのかを公開する。
    //
    //    証明項は witness のまま journal に出さないが、それだけでは検証者に
    //    何も伝わらない。定理の **型**（= statement）の正準ダイジェストを出す。
    //    検証者は自分の書いた命題から同じダイジェストを再計算して突き合わせる。
    let statement = stmt::statement_digest(&export_file, &target_name)
        .expect("target theorem not found in the export file");

    let cycles_stmt = env::cycle_count();

    let suffix_declars = num_declars - num_compact_prelude_declars as u64;
    // `certified` では「Stage 1 が検査した分 + ここで検査した分」。
    // `assumed` では prelude は前提なので、ここで検査した分だけを数える。
    let num_checked = suffix_declars
        + if certified { checked_prelude.len() as u64 } else { 0 };
    env::log(&format!(
        "thmcheck: receipt={} auth={}(decode={} merkle={} names={}) compact={} parse={} check={} stmt={} total={} | env={} mode={} checked={} (stage2 prelude {}/{})",
        cycles_verify - start,
        cycles_merkle - cycles_verify,
        auth_decode,
        auth_merkle,
        (cycles_merkle - cycles_verify).saturating_sub(auth_decode + auth_merkle),
        cycles_compact - cycles_merkle,
        cycles_parse - cycles_compact,
        cycles_check - cycles_parse,
        cycles_stmt - cycles_check,
        cycles_stmt - start,
        if certified { "certified" } else { "assumed" },
        if sparse_mode { "sparse" } else { "full" },
        num_checked,
        num_compact_prelude_declars,
        num_prelude_declars,
    ));

    // journal に出るのはここまで。証明項（suffix）そのものは一切出さない。
    //
    // 環境の信頼モードを必ず出す。これが無いと、prelude を前提にしただけの証明を
    // 「Stage 1 付き」と偽って通せてしまう。`assumed` では image id をゼロにする。
    env::commit(&(
        certified,
        if certified { PINNED_ENVCHECK_ID } else { [0u32; 8] },
        prelude_digest,
        record_root,
        names_root,
        target_name,
        statement,
        num_checked,
        skipped_axioms,
    ));
}
