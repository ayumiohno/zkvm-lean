//! [lean-zkvm patch] 型検査中にどの宣言が読まれたかを記録する。
//!
//! Merkle 木による遅延ロード（「触った宣言だけを witness で渡す」）に
//! どれだけの効果があるかを見積もるための計測。`touch_trace` feature を
//! 有効にしたときだけ組み込まれる。

use std::cell::RefCell;

thread_local! {
    /// 型（宣言そのもの）を読まれた宣言のインデックス。
    static TOUCHED: RefCell<crate::util::FxHashSet<usize>> = RefCell::new(Default::default());
    /// さらに値（定義本体）まで展開された宣言のインデックス。
    static UNFOLDED: RefCell<crate::util::FxHashSet<usize>> = RefCell::new(Default::default());
    /// export 側 DAG から読まれた項ノードのインデックス。
    /// Merkle 木を項ノード単位で張る場合、開く葉の数がこれになる。
    static NODES: RefCell<crate::util::FxHashSet<u64>> = RefCell::new(Default::default());
}

/// 読まれた項ノードを記録する。`kind` は Name/Level/Expr の別。
pub(crate) fn note_node(kind: u8, idx: usize) {
    NODES.with(|s| {
        s.borrow_mut().insert(((kind as u64) << 56) | idx as u64);
    })
}

pub(crate) fn note_touched(idx: usize) {
    TOUCHED.with(|s| {
        s.borrow_mut().insert(idx);
    })
}

pub(crate) fn note_unfolded(idx: usize) {
    UNFOLDED.with(|s| {
        s.borrow_mut().insert(idx);
    })
}

/// 記録を消す。計測区間の直前に呼ぶ。
pub fn reset() {
    TOUCHED.with(|s| s.borrow_mut().clear());
    UNFOLDED.with(|s| s.borrow_mut().clear());
    NODES.with(|s| s.borrow_mut().clear());
}

/// (型を読まれた宣言数, 値まで展開された宣言数)
pub fn counts() -> (usize, usize) {
    (TOUCHED.with(|s| s.borrow().len()), UNFOLDED.with(|s| s.borrow().len()))
}

/// export 側 DAG から読まれた項ノードの数。
pub fn node_count() -> usize {
    NODES.with(|s| s.borrow().len())
}

/// 読まれた項ノードの (種別, インデックス) 一覧。
/// 種別は 0=Name, 1=Level, 2=Expr。
pub fn node_indices() -> Vec<(u8, u32)> {
    NODES.with(|s| s.borrow().iter().map(|v| ((v >> 56) as u8, (v & 0xffff_ffff) as u32)).collect())
}

/// 型を読まれた宣言のインデックス一覧（昇順）。
pub fn touched_indices() -> Vec<usize> {
    let mut v: Vec<usize> = TOUCHED.with(|s| s.borrow().iter().copied().collect());
    v.sort_unstable();
    v
}

/// 型検査の主要操作の回数。
///
/// `check` のサイクル数がどこに消えているかは、回数を数えて 1 回あたりのコストと
/// 突き合わせるのが唯一まともに機能する手順だった（Merkle でそう学んだ）。
pub mod ops {
    use std::cell::Cell;

    macro_rules! counters {
        ($($name:ident),* $(,)?) => {
            thread_local! {
                $(pub static $name: Cell<u64> = const { Cell::new(0) };)*
            }
            /// すべて 0 に戻す。
            pub fn reset() { $($name.with(|c| c.set(0));)* }
            /// (名前, 回数) の一覧。
            pub fn read() -> Vec<(&'static str, u64)> {
                vec![$((stringify!($name), $name.with(|c| c.get()))),*]
            }
        };
    }

    counters!(
        INFER,       // 型推論の呼び出し
        WHNF,        // 弱頭正規形への簡約
        DEF_EQ,      // 定義上の等しさの判定
        DELTA,       // 定義の展開（delta 簡約）
        REDUCE_REC,  // 再帰子の適用（iota 簡約）
        REDUCE_QUOT, // Quot の簡約
        REDUCE_PROJ, // 射影の簡約
        NAT_OP,      // Nat 拡張（GMP による literal 演算）
        CACHE_HIT,   // def_eq のキャッシュヒット
    );

    #[inline(always)]
    pub(crate) fn bump(c: &'static std::thread::LocalKey<Cell<u64>>) {
        c.with(|v| v.set(v.get() + 1));
    }
}
