//! statement（定理の型）の正準ダイジェスト。
//!
//! export file の中で項は整数インデックスで共有されるため、同じ命題でも
//! ファイルが違えばインデックスは変わる。公開のコミットメントに使うには、
//! **項の構造だけから決まる**表現が要る。
//!
//! ここでは項を深さ優先で正準バイト列に符号化し、SHA-256 を取る。
//! guest と検証者側のツールが同じ関数を使うので、検証者は自分で書いた
//! statement から同じダイジェストを再計算して突き合わせられる。

pub mod policy;
pub mod merkle;

use nanoda_lib::expr::{BinderStyle, Expr};
use nanoda_lib::level::Level;
use nanoda_lib::name::Name;
use nanoda_lib::util::{ExportFile, ExprPtr, LevelPtr, LevelsPtr, NamePtr, TcCtx};
use sha2::{Digest as _, Sha256};

/// 項の構造から決まる 32 バイトのダイジェスト。
pub type StmtDigest = [u8; 32];

/// Return whether a sorted, duplicate-free Stage 1 certificate covers every
/// prelude declaration observed by Stage 2.
pub fn certificate_covers(checked: &[u32], touched: &[usize], prelude_len: usize) -> bool {
    checked.windows(2).all(|w| w[0] < w[1])
        && checked.iter().all(|idx| (*idx as usize) < prelude_len)
        && touched
            .iter()
            .filter(|idx| **idx < prelude_len)
            .all(|idx| u32::try_from(*idx).is_ok_and(|idx| checked.binary_search(&idx).is_ok()))
}

/// Name-based certificate coverage used after sparse compaction has changed
/// declaration insertion indices.
pub fn name_certificate_covers(checked: &[String], touched: &[String]) -> bool {
    checked.windows(2).all(|w| w[0] < w[1])
        && touched.iter().all(|name| checked.binary_search(name).is_ok())
}

pub fn suffix_avoids_public_names(public_names: &[String], suffix_names: &[String]) -> bool {
    public_names.windows(2).all(|w| w[0] < w[1])
        && suffix_names
            .iter()
            .all(|name| public_names.binary_search(name).is_err())
}

/// 定理の名前からその型（＝ statement）の正準ダイジェストを求める。
/// 見つからなければ `None`。
pub fn statement_digest(export_file: &ExportFile<'_>, target: &str) -> Option<StmtDigest> {
    export_file.with_ctx(|ctx| {
        for declar in export_file.declars.values() {
            let info = declar.info();
            if name_to_string(ctx, info.name) == target {
                let mut enc = Encoder::default();
                enc.expr(ctx, info.ty);
                return Some(enc.finish());
            }
        }
        None
    })
}

/// 名前を `Foo.bar.baz` の形の文字列にする。
pub fn name_to_string<'t, 'p: 't>(ctx: &TcCtx<'t, 'p>, p: NamePtr<'t>) -> String {
    match ctx.read_name(p) {
        Name::Anon => String::new(),
        Name::Str(pre, s, _) => join(name_to_string(ctx, pre), &ctx.read_string(s).to_string()),
        Name::Num(pre, i, _) => join(name_to_string(ctx, pre), &i.to_string()),
    }
}

fn join(prefix: String, last: &str) -> String {
    if prefix.is_empty() {
        last.to_string()
    } else {
        format!("{prefix}.{last}")
    }
}

#[cfg(test)]
mod certificate_tests {
    use super::certificate_covers;

    #[test]
    fn accepts_covered_dependencies() {
        assert!(certificate_covers(&[1, 4, 7], &[4, 7, 12], 10));
    }

    #[test]
    fn rejects_a_missing_dependency() {
        assert!(!certificate_covers(&[1, 7], &[4, 7, 12], 10));
    }

    #[test]
    fn rejects_noncanonical_certificate() {
        assert!(!certificate_covers(&[4, 1], &[1], 10));
        assert!(!certificate_covers(&[1, 1], &[1], 10));
        assert!(!certificate_covers(&[1, 10], &[1], 10));
    }

    #[test]
    fn name_certificate_rejects_a_missing_dependency() {
        let checked = vec!["A".to_string(), "C".to_string()];
        assert!(super::name_certificate_covers(&checked, &["C".to_string()]));
        assert!(!super::name_certificate_covers(&checked, &["B".to_string()]));
    }

    #[test]
    fn rejects_private_shadowing_of_an_omitted_public_declaration() {
        let public = vec!["Public.f".to_string(), "Public.g".to_string()];
        assert!(super::suffix_avoids_public_names(&public, &["Private.proof".to_string()]));
        assert!(!super::suffix_avoids_public_names(&public, &["Public.g".to_string()]));
    }
}

/// 正準バイト列を組み立てる。
///
/// 各構成子に固有のタグを振り、子を再帰的に書き出す。共有（DAG）は展開
/// されるので、極端に共有の多い項では大きくなりうるが、statement は
/// 小さいので実用上問題にならない。
#[derive(Default)]
struct Encoder {
    buf: Vec<u8>,
}

impl Encoder {
    fn finish(self) -> StmtDigest {
        let mut h = Sha256::new();
        h.update(&self.buf);
        h.finalize().into()
    }

    fn tag(&mut self, t: u8) {
        self.buf.push(t);
    }

    fn bytes(&mut self, b: &[u8]) {
        self.buf.extend_from_slice(&(b.len() as u32).to_le_bytes());
        self.buf.extend_from_slice(b);
    }

    fn name<'t, 'p: 't>(&mut self, ctx: &TcCtx<'t, 'p>, p: NamePtr<'t>) {
        match ctx.read_name(p) {
            Name::Anon => self.tag(0x00),
            Name::Str(pre, s, _) => {
                self.tag(0x01);
                self.name(ctx, pre);
                let s = ctx.read_string(s).to_string();
                self.bytes(s.as_bytes());
            }
            Name::Num(pre, i, _) => {
                self.tag(0x02);
                self.name(ctx, pre);
                self.buf.extend_from_slice(&i.to_le_bytes());
            }
        }
    }

    fn level<'t, 'p: 't>(&mut self, ctx: &TcCtx<'t, 'p>, p: LevelPtr<'t>) {
        match ctx.read_level(p) {
            Level::Zero => self.tag(0x10),
            Level::Succ(l, _) => {
                self.tag(0x11);
                self.level(ctx, l);
            }
            Level::Max(a, b, _) => {
                self.tag(0x12);
                self.level(ctx, a);
                self.level(ctx, b);
            }
            Level::IMax(a, b, _) => {
                self.tag(0x13);
                self.level(ctx, a);
                self.level(ctx, b);
            }
            Level::Param(n, _) => {
                self.tag(0x14);
                self.name(ctx, n);
            }
        }
    }

    fn levels<'t, 'p: 't>(&mut self, ctx: &TcCtx<'t, 'p>, p: LevelsPtr<'t>) {
        let ls = ctx.read_levels(p);
        self.buf.extend_from_slice(&(ls.len() as u32).to_le_bytes());
        for l in ls.iter() {
            self.level(ctx, *l);
        }
    }

    fn binder_style(&mut self, s: BinderStyle) {
        self.tag(match s {
            BinderStyle::Default => 0,
            BinderStyle::Implicit => 1,
            BinderStyle::StrictImplicit => 2,
            BinderStyle::InstanceImplicit => 3,
        });
    }

    fn expr<'t, 'p: 't>(&mut self, ctx: &TcCtx<'t, 'p>, p: ExprPtr<'t>) {
        match ctx.read_expr(p) {
            Expr::Var { dbj_idx, .. } => {
                self.tag(0x20);
                self.buf.extend_from_slice(&dbj_idx.to_le_bytes());
            }
            Expr::Sort { level, .. } => {
                self.tag(0x21);
                self.level(ctx, level);
            }
            Expr::Const { name, levels, .. } => {
                self.tag(0x22);
                self.name(ctx, name);
                self.levels(ctx, levels);
            }
            Expr::App { fun, arg, .. } => {
                self.tag(0x23);
                self.expr(ctx, fun);
                self.expr(ctx, arg);
            }
            Expr::Lambda { binder_name, binder_style, binder_type, body, .. } => {
                self.tag(0x24);
                self.name(ctx, binder_name);
                self.binder_style(binder_style);
                self.expr(ctx, binder_type);
                self.expr(ctx, body);
            }
            Expr::Pi { binder_name, binder_style, binder_type, body, .. } => {
                self.tag(0x25);
                self.name(ctx, binder_name);
                self.binder_style(binder_style);
                self.expr(ctx, binder_type);
                self.expr(ctx, body);
            }
            Expr::Let { binder_name, binder_type, val, body, nondep, .. } => {
                self.tag(0x26);
                self.name(ctx, binder_name);
                self.expr(ctx, binder_type);
                self.expr(ctx, val);
                self.expr(ctx, body);
                self.tag(nondep as u8);
            }
            Expr::Proj { ty_name, idx, structure, .. } => {
                self.tag(0x27);
                self.name(ctx, ty_name);
                self.buf.extend_from_slice(&(idx as u64).to_le_bytes());
                self.expr(ctx, structure);
            }
            Expr::NatLit { ptr, .. } => {
                self.tag(0x28);
                let n = ctx.read_bignum(ptr).expect("nat literal not found").to_bytes_le();
                self.bytes(&n);
            }
            Expr::StringLit { ptr, .. } => {
                self.tag(0x29);
                let s = ctx.read_string(ptr).to_string();
                self.bytes(s.as_bytes());
            }
            // 自由変数は閉じた statement には現れない。
            Expr::Local { .. } => panic!("statement contains a free variable"),
        }
    }
}

/// ダイジェストを16進文字列にする。
pub fn hex(d: &StmtDigest) -> String {
    d.iter().map(|b| format!("{b:02x}")).collect()
}
