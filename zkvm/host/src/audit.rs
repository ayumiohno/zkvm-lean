//! Verifier-side auditing of a public environment.
//!
//! **This lives in the host, not in `stmt`.** The guest links `stmt` and its ELF
//! carries panic locations, so editing that crate at all — even adding dead code —
//! moves line numbers and changes the guest image ID. Receipts already published
//! would then need re-proving to satisfy a purely verifier-side hardening. Nothing
//! here is needed inside the guest, so nothing here goes there.
//!
//! What it provides:
//!
//!   - structural name keys, because `stmt::name_to_string` flattens distinct kernel
//!     names onto one string and these tests accept on a hit;
//!   - the constants a statement refers to, which a statement digest pins by name
//!     only;
//!   - a fingerprint for an allowlisted axiom, because the parser admits an axiom on
//!     its name without comparing the type.

use nanoda_lib::expr::Expr;
use nanoda_lib::level::Level;
use nanoda_lib::name::Name;
use nanoda_lib::util::{ExportFile, ExprPtr, LevelsPtr, NamePtr, TcCtx};
use sha2::{Digest as _, Sha256};

/// A kernel `Name`, encoded structurally.
///
/// `Str(Anon, "Foo.bar")` and `Str(Str(Anon, "Foo"), "bar")` are distinct names that
/// `name_to_string` renders identically, as are `Num(pre, 1)` and `Str(pre, "1")`.
/// Membership tests that *accept* on a hit have to compare these instead.
pub type NameKey = Vec<u8>;

fn push_bytes(buf: &mut Vec<u8>, b: &[u8]) {
    buf.extend_from_slice(&(b.len() as u32).to_le_bytes());
    buf.extend_from_slice(b);
}

fn encode_name<'t, 'p: 't>(ctx: &TcCtx<'t, 'p>, p: NamePtr<'t>, buf: &mut Vec<u8>) {
    match ctx.read_name(p) {
        Name::Anon => buf.push(0x00),
        Name::Str(pre, s, _) => {
            buf.push(0x01);
            encode_name(ctx, pre, buf);
            push_bytes(buf, ctx.read_string(s).to_string().as_bytes());
        }
        Name::Num(pre, i, _) => {
            buf.push(0x02);
            encode_name(ctx, pre, buf);
            buf.extend_from_slice(&i.to_le_bytes());
        }
    }
}

fn encode_levels<'t, 'p: 't>(ctx: &TcCtx<'t, 'p>, p: LevelsPtr<'t>, buf: &mut Vec<u8>) {
    let ls = ctx.read_levels(p);
    buf.extend_from_slice(&(ls.len() as u32).to_le_bytes());
    for l in ls.iter() {
        encode_level(ctx, *l, buf);
    }
}

fn encode_level<'t, 'p: 't>(ctx: &TcCtx<'t, 'p>, p: nanoda_lib::util::LevelPtr<'t>, buf: &mut Vec<u8>) {
    match ctx.read_level(p) {
        Level::Zero => buf.push(0x10),
        Level::Succ(l, _) => {
            buf.push(0x11);
            encode_level(ctx, l, buf);
        }
        Level::Max(a, b, _) => {
            buf.push(0x12);
            encode_level(ctx, a, buf);
            encode_level(ctx, b, buf);
        }
        Level::IMax(a, b, _) => {
            buf.push(0x13);
            encode_level(ctx, a, buf);
            encode_level(ctx, b, buf);
        }
        Level::Param(n, _) => {
            buf.push(0x14);
            encode_name(ctx, n, buf);
        }
    }
}

pub fn name_key<'t, 'p: 't>(ctx: &TcCtx<'t, 'p>, p: NamePtr<'t>) -> NameKey {
    let mut buf = Vec::new();
    encode_name(ctx, p, &mut buf);
    buf
}

/// Every declaration name in an export, structurally encoded, sorted and deduplicated.
pub fn name_keys(export_file: &ExportFile<'_>) -> Vec<NameKey> {
    let mut keys: Vec<NameKey> = export_file
        .with_ctx(|ctx| export_file.declars.values().map(|d| name_key(ctx, d.info().name)).collect());
    keys.sort();
    keys.dedup();
    keys
}

/// How many declarations flatten to `target`.
///
/// More than one means the string does not identify a declaration, and a check that
/// resolves one by name must refuse rather than pick.
pub fn count_declars_named(export_file: &ExportFile<'_>, target: &str) -> usize {
    export_file.with_ctx(|ctx| {
        export_file
            .declars
            .values()
            .filter(|d| stmt::name_to_string(ctx, d.info().name) == target)
            .count()
    })
}

/// The constants a declaration's type refers to: `Expr::Const` names together with
/// the structure names of `Expr::Proj`.
///
/// A statement digest pins these names but not their definitions, so a verifier has
/// to establish separately that the public environment is what resolves them.
pub fn statement_constants(export_file: &ExportFile<'_>, target: &str) -> Option<Vec<NameKey>> {
    export_file.with_ctx(|ctx| {
        for declar in export_file.declars.values() {
            let info = declar.info();
            if stmt::name_to_string(ctx, info.name) == target {
                let mut out = Vec::new();
                collect(ctx, info.ty, &mut out);
                out.sort();
                out.dedup();
                return Some(out);
            }
        }
        None
    })
}

fn collect<'t, 'p: 't>(ctx: &TcCtx<'t, 'p>, p: ExprPtr<'t>, out: &mut Vec<NameKey>) {
    match ctx.read_expr(p) {
        Expr::Const { name, .. } => out.push(name_key(ctx, name)),
        Expr::App { fun, arg, .. } => {
            collect(ctx, fun, out);
            collect(ctx, arg, out);
        }
        Expr::Lambda { binder_type, body, .. } | Expr::Pi { binder_type, body, .. } => {
            collect(ctx, binder_type, out);
            collect(ctx, body, out);
        }
        Expr::Let { binder_type, val, body, .. } => {
            collect(ctx, binder_type, out);
            collect(ctx, val, out);
            collect(ctx, body, out);
        }
        Expr::Proj { ty_name, structure, .. } => {
            out.push(name_key(ctx, ty_name));
            collect(ctx, structure, out);
        }
        Expr::Var { .. } | Expr::Sort { .. } | Expr::NatLit { .. } | Expr::StringLit { .. } => {}
        Expr::Local { .. } => panic!("a statement must not contain a free variable"),
    }
}

/// A canonical encoding of an expression.
///
/// Only ever compared against itself — both sides of every test here are produced by
/// this function — so it does not have to agree with the guest's encoding, and is
/// kept separate from `stmt` for the reason given at the top of this file.
fn encode_expr<'t, 'p: 't>(ctx: &TcCtx<'t, 'p>, p: ExprPtr<'t>, buf: &mut Vec<u8>) {
    match ctx.read_expr(p) {
        Expr::Var { dbj_idx, .. } => {
            buf.push(0x20);
            buf.extend_from_slice(&dbj_idx.to_le_bytes());
        }
        Expr::Sort { level, .. } => {
            buf.push(0x21);
            encode_level(ctx, level, buf);
        }
        Expr::Const { name, levels, .. } => {
            buf.push(0x22);
            encode_name(ctx, name, buf);
            encode_levels(ctx, levels, buf);
        }
        Expr::App { fun, arg, .. } => {
            buf.push(0x23);
            encode_expr(ctx, fun, buf);
            encode_expr(ctx, arg, buf);
        }
        Expr::Lambda { binder_name, binder_style, binder_type, body, .. } => {
            buf.push(0x24);
            encode_name(ctx, binder_name, buf);
            buf.push(binder_style as u8);
            encode_expr(ctx, binder_type, buf);
            encode_expr(ctx, body, buf);
        }
        Expr::Pi { binder_name, binder_style, binder_type, body, .. } => {
            buf.push(0x25);
            encode_name(ctx, binder_name, buf);
            buf.push(binder_style as u8);
            encode_expr(ctx, binder_type, buf);
            encode_expr(ctx, body, buf);
        }
        Expr::Let { binder_name, binder_type, val, body, .. } => {
            buf.push(0x26);
            encode_name(ctx, binder_name, buf);
            encode_expr(ctx, binder_type, buf);
            encode_expr(ctx, val, buf);
            encode_expr(ctx, body, buf);
        }
        Expr::Proj { ty_name, idx, structure, .. } => {
            buf.push(0x27);
            encode_name(ctx, ty_name, buf);
            buf.extend_from_slice(&(idx as u64).to_le_bytes());
            encode_expr(ctx, structure, buf);
        }
        Expr::NatLit { ptr, .. } => {
            buf.push(0x28);
            push_bytes(buf, &ctx.read_bignum(ptr).expect("nat literal not found").to_bytes_le());
        }
        Expr::StringLit { ptr, .. } => {
            buf.push(0x29);
            push_bytes(buf, ctx.read_string(ptr).to_string().as_bytes());
        }
        Expr::Local { .. } => panic!("an exported declaration must not contain a free variable"),
    }
}

/// What a declaration *is*: its kind, structural name, universe parameters, type, and
/// defining value where it has one.
///
/// Two exports agreeing on this for a name agree on what that name means.
///
/// **Inductives, constructors and recursors are covered by kind, name, universe
/// parameters and type only** — their remaining payload is private to nanoda. In an
/// export the constructors and the recursor are separate declarations with their own
/// types, so the family is still compared member by member; what is not compared is a
/// recursor's reduction rules.
pub fn declar_fingerprint<'t, 'p: 't>(
    ctx: &TcCtx<'t, 'p>,
    declar: &nanoda_lib::env::Declar<'t>,
) -> [u8; 32] {
    use nanoda_lib::env::Declar;
    let (tag, val) = match declar {
        Declar::Axiom { .. } => (0x40u8, None),
        Declar::Quot { .. } => (0x41, None),
        Declar::Theorem { val, .. } => (0x42, Some(*val)),
        Declar::Definition { val, .. } => (0x43, Some(*val)),
        Declar::Opaque { val, .. } => (0x44, Some(*val)),
        Declar::Inductive(_) => (0x45, None),
        Declar::Constructor(_) => (0x46, None),
        Declar::Recursor(_) => (0x47, None),
    };
    let info = declar.info();
    let mut buf = vec![tag];
    encode_name(ctx, info.name, &mut buf);
    encode_levels(ctx, info.uparams, &mut buf);
    encode_expr(ctx, info.ty, &mut buf);
    if let Some(v) = val {
        buf.push(0x01);
        encode_expr(ctx, v, &mut buf);
    } else {
        buf.push(0x00);
    }
    let mut h = Sha256::new();
    h.update(&buf);
    h.finalize().into()
}

/// Every declaration of an export, keyed by its structural name.
pub fn fingerprints(export_file: &ExportFile<'_>) -> Vec<(NameKey, [u8; 32])> {
    let mut v: Vec<(NameKey, [u8; 32])> = export_file.with_ctx(|ctx| {
        export_file
            .declars
            .values()
            .map(|d| (name_key(ctx, d.info().name), declar_fingerprint(ctx, d)))
            .collect()
    });
    v.sort_by(|a, b| a.0.cmp(&b.0));
    v
}

/// The fingerprint of an allowlisted axiom, refusing anything that is not an axiom.
pub fn axiom_fingerprint(export_file: &ExportFile<'_>, target: &str) -> Option<[u8; 32]> {
    export_file.with_ctx(|ctx| {
        for declar in export_file.declars.values() {
            if stmt::name_to_string(ctx, declar.info().name) != target {
                continue;
            }
            if !matches!(declar, nanoda_lib::env::Declar::Axiom { .. }) {
                return None;
            }
            return Some(declar_fingerprint(ctx, declar));
        }
        None
    })
}

/// The pinned fingerprint of each allowlisted axiom.
///
/// An allowlist by name does not pin an axiom system: the parser admits a declaration
/// named `propext` whose type is `∀ p : Prop, p`, and that proves everything. These
/// values were taken from Lean 4.34.0-rc2 exports and cross-checked across two
/// independent preludes.
///
/// `None` means no sample was available to pin. Such an axiom is **rejected** where it
/// appears, rather than accepted unaudited.
pub const AXIOM_FINGERPRINTS: [(&str, Option<[u8; 32]>); 4] = [
    ("propext", Some(PROPEXT)),
    ("Classical.choice", Some(CLASSICAL_CHOICE)),
    ("Quot.sound", Some(QUOT_SOUND)),
    // No export in this repository declares it, so there is nothing to pin against.
    ("Lean.trustCompiler", None),
];

const PROPEXT: [u8; 32] = [134, 94, 109, 247, 26, 197, 2, 23, 204, 102, 164, 130, 137, 76, 80, 102, 209, 156, 126, 96, 67, 104, 13, 41, 24, 195, 173, 136, 248, 207, 55, 187];
const CLASSICAL_CHOICE: [u8; 32] = [161, 231, 205, 219, 234, 250, 126, 21, 184, 228, 122, 92, 168, 86, 117, 39, 121, 163, 86, 28, 64, 75, 35, 12, 180, 135, 219, 148, 19, 37, 80, 46];
const QUOT_SOUND: [u8; 32] = [75, 9, 15, 1, 147, 195, 206, 171, 39, 164, 50, 202, 208, 75, 223, 193, 6, 239, 63, 130, 132, 30, 128, 73, 158, 100, 151, 98, 155, 165, 15, 18];

/// The pinned fingerprint for `name`, if there is one.
pub fn pinned(name: &str) -> Option<[u8; 32]> {
    AXIOM_FINGERPRINTS.iter().find(|(n, _)| *n == name).and_then(|(_, d)| *d)
}
