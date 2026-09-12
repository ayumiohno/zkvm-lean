//! 型検査のポリシー。
//!
//! **これは witness から受け取ってはいけない。**
//!
//! nanoda の `Config` には `unsafe_permit_all_axioms` があり、
//! `axiom_permitted` は
//!
//! ```text
//! self.config.unsafe_permit_all_axioms || permitted_axioms.contains(..)
//! ```
//!
//! と短絡する。さらに排他チェックは `Config::try_from(&Path)` の中にしかなく、
//! guest が使う `serde_json::from_str` 経路では働かない。
//!
//! したがってポリシーを private input から読むと、prover は同じ guest image の
//! まま `unsafe_permit_all_axioms: true` を渡して `axiom cheat : False` を
//! 通せてしまう。ホスト側に置いた安全な allowlist は、ホストを信頼しない設計では
//! セキュリティ境界にならない。
//!
//! そのため guest 内に固定し、危険な組み合わせでないことを起動時に確かめる。

use nanoda_lib::util::Config;

/// guest に焼き込む唯一のポリシー。
const POLICY_JSON: &str = r#"{
    "use_stdin": false,
    "num_threads": 1,
    "permitted_axioms": ["propext", "Classical.choice", "Quot.sound", "Lean.trustCompiler"],
    "unpermitted_axiom_hard_error": false,
    "unsafe_permit_all_axioms": false,
    "nat_extension": true,
    "string_extension": true,
    "print_success_message": false
}"#;

/// 検証者が期待する axiom の allowlist（journal の突き合わせにも使う）。
pub const PERMITTED_AXIOMS: [&str; 4] =
    ["propext", "Classical.choice", "Quot.sound", "Lean.trustCompiler"];

/// 固定ポリシーを返す。危険な設定になっていれば panic する。
///
/// zkVM では panic = 証明が生成されない、なので安全側に倒れる。
pub fn config() -> Config {
    let cfg: Config = serde_json::from_str(POLICY_JSON).expect("固定ポリシーが壊れている");

    // 焼き込んだ JSON が将来壊れても検知できるように、実際の値を確かめる。
    assert!(!cfg.unsafe_permit_all_axioms, "unsafe_permit_all_axioms は必ず false");
    assert_eq!(cfg.num_threads, 1, "zkVM にスレッドはない");
    let allow = cfg.permitted_axioms.as_ref().expect("allowlist が空だと全 axiom が拒否される");
    assert_eq!(allow.len(), PERMITTED_AXIOMS.len(), "allowlist の件数が合わない");
    for a in PERMITTED_AXIOMS {
        assert!(allow.iter().any(|x| x == a), "allowlist に {a} がない");
    }
    cfg
}
