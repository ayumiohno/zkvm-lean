use crate::env::{ConstructorData, Declar, DeclarInfo, InductiveData, Notation, RecursorData, ReducibilityHint};
use crate::expr::{BinderStyle, Expr};
use crate::hash64;
use crate::level::Level;
use crate::name::Name;
use crate::util::{
    new_fx_hash_map, new_fx_hash_set, new_fx_index_map, BigUintPtr, Config, DagMarker, ExprPtr, FxHashMap, FxIndexMap,
    LeanDag, LevelPtr, LevelsPtr, NamePtr, StringPtr,
};
use num_bigint::BigUint;
use serde::de::{Error as DeError, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use std::borrow::Cow;
use std::error::Error;
use std::fmt;
use std::io::BufRead;
use std::sync::Arc;

fn check_semver<'a>(meta: &FileMeta<'a>) -> Result<(), Box<dyn Error>> {
    const MIN_SEMVER: semver::Version = semver::Version::new(3, 1, 0);
    const MAX_SEMVER: semver::Version = semver::Version::new(3, 2, 0);
    let export_file_semver = semver::Version::parse(&meta.format.version)?;
    if export_file_semver < MIN_SEMVER {
        return Err(Box::from(format!(
            "export format version is less than the minimum supported version. Found {}, but min supported is {}",
            export_file_semver, MIN_SEMVER
        )));
    } else if export_file_semver >= MAX_SEMVER {
        return Err(Box::from(format!(
            "export format version is greater than the maximum supported version. Found {}, but max (exclusive) supported is {}",
            export_file_semver, MAX_SEMVER
        )));
    } else {
        Ok(())
    }
}

pub struct Parser<'a, R: BufRead> {
    buf_reader: R,
    line_num: usize,
    dag: LeanDag<'a>,
    declars: FxIndexMap<NamePtr<'a>, Declar<'a>>,
    notations: FxHashMap<NamePtr<'a>, Notation<'a>>,
    config: Config,
    /// Tracks axiom names that were found in the export file, but not white-listed,
    /// for use when `unpermitted_axiom_hard_error: false`
    skipped: Vec<String>,
    mutual_block_sizes: FxHashMap<NamePtr<'a>, (usize, usize)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub struct LeanMeta<'a> {
    version: Cow<'a, str>,
    githash: Cow<'a, str>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub struct ExporterMeta<'a> {
    name: Cow<'a, str>,
    version: Cow<'a, str>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub struct FormatMeta<'a> {
    version: Cow<'a, str>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct FileMeta<'a> {
    lean: LeanMeta<'a>,
    exporter: ExporterMeta<'a>,
    format: FormatMeta<'a>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub enum BackRef {
    #[serde(alias = "in")]
    In(u32),
    #[serde(alias = "il")]
    Il(u32),
    #[serde(alias = "ie")]
    Ie(u32),
}

impl BackRef {
    fn assert_in(self, (idx, inserted): (usize, bool)) {
        if !inserted {
            panic!("Attempted to insert duplicate Name");
        }
        let lhs = u32::try_from(idx).unwrap();
        if self != BackRef::In(lhs) {
            eprintln!(
                "Declined: Name back-reference mismatch, expected {:?}, found {:?}. Back-refs must be continuous.",
                BackRef::In(lhs),
                self
            );
            std::process::exit(2);
        }
    }

    fn assert_il(self, (idx, inserted): (usize, bool)) {
        if !inserted {
            panic!("Attempted to insert duplicate Level");
        }
        let lhs = u32::try_from(idx).unwrap();
        if self != BackRef::Il(lhs) {
            eprintln!(
                "Declined: Level back-reference mismatch, expected {:?}, found {:?}. Back-refs must be continuous.",
                BackRef::Il(lhs),
                self
            );
            std::process::exit(2);
        }
    }

    fn assert_ie(self, (idx, inserted): (usize, bool)) {
        if !inserted {
            panic!("Attempted to insert duplicate Expr");
        }
        let lhs = u32::try_from(idx).unwrap();
        if self != BackRef::Ie(lhs) {
            eprintln!(
                "Declined: Expr back-reference mismatch, expected {:?}, found {:?}. Back-refs must be continuous.",
                BackRef::Ie(lhs),
                self
            );
            std::process::exit(2);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct ExportJsonObject<'a> {
    #[serde(flatten)]
    val: ExportJsonVal<'a>,
    #[serde(flatten)]
    i: Option<BackRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub enum DefinitionSafety {
    #[serde(rename = "unsafe")]
    Unsafe,
    #[serde(rename = "safe")]
    Safe,
    #[serde(rename = "partial")]
    Partial,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub enum QuotKind {
    #[serde(rename = "type")]
    Ty,
    #[serde(rename = "ctor")]
    Ctor,
    #[serde(rename = "lift")]
    Lift,
    #[serde(rename = "ind")]
    Ind,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub struct RecursorRule {
    ctor: u32,
    nfields: u16,
    rhs: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub struct IndInfo {
    name: u32,
    #[serde(rename = "levelParams")]
    uparams: Vec<u32>,
    #[serde(rename = "type")]
    ty: u32,
    all: Vec<u32>,
    ctors: Vec<u32>,
    #[serde(rename = "isRec")]
    is_rec: bool,
    #[serde(rename = "isReflexive")]
    is_reflexive: bool,
    #[serde(rename = "numIndices")]
    num_indices: u16,
    #[serde(rename = "numNested")]
    num_nested: u16,
    #[serde(rename = "numParams")]
    num_params: u16,
    #[serde(rename = "isUnsafe")]
    is_unsafe: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub struct Constructor {
    name: u32,
    #[serde(rename = "levelParams")]
    uparams: Vec<u32>,
    #[serde(rename = "type")]
    ty: u32,
    #[serde(rename = "isUnsafe")]
    is_unsafe: bool,
    cidx: u16,
    #[serde(rename = "numParams")]
    num_params: u16,
    #[serde(rename = "numFields")]
    num_fields: u16,
    induct: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub struct Recursor {
    name: u32,
    #[serde(rename = "levelParams")]
    uparams: Vec<u32>,
    #[serde(rename = "type")]
    ty: u32,
    #[serde(rename = "isUnsafe")]
    is_unsafe: bool,
    #[serde(rename = "numParams")]
    num_params: u16,
    #[serde(rename = "numIndices")]
    num_indices: u16,
    #[serde(rename = "numMotives")]
    num_motives: u16,
    #[serde(rename = "numMinors")]
    num_minors: u16,
    rules: Vec<RecursorRule>,
    all: Vec<u32>,
    k: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub enum ExportJsonVal<'a> {
    // The exporter metadata, incl. info about the lean, exporter, and format versions used
    // to create the export file.
    #[serde(rename = "meta")]
    Metadata(FileMeta<'a>),
    #[serde(rename = "str")]
    NameStr { pre: u32, str: Cow<'a, str> },
    #[serde(rename = "num")]
    NameNum { pre: u32, i: u32 },
    #[serde(rename = "succ")]
    LevelSucc(u32),
    #[serde(rename = "max")]
    LevelMax([u32; 2]),
    #[serde(rename = "imax")]
    LevelIMax([u32; 2]),
    #[serde(rename = "param")]
    LevelParam(u32),
    #[serde(
        rename = "natVal",
        deserialize_with = "deserialize_biguint_from_string",
        serialize_with = "serialize_biguint_as_string"
    )]
    NatLit(BigUint),
    #[serde(rename = "strVal")]
    StrLit(Cow<'a, str>),
    #[serde(rename = "mdata")]
    ExprMData { expr: u32, data: serde_json::Value },
    #[serde(rename = "letE")]
    ExprLet {
        name: u32,
        #[serde(rename = "type")]
        ty: u32,
        value: u32,
        body: u32,
        nondep: bool,
    },
    #[serde(rename = "const")]
    ExprConst {
        name: u32,
        #[serde(rename = "us")]
        levels: Vec<u32>,
    },
    #[serde(rename = "app")]
    ExprApp {
        #[serde(rename = "fn")]
        fun: u32,
        arg: u32,
    },
    #[serde(rename = "forallE")]
    ExprPi {
        #[serde(rename = "name")]
        binder_name: u32,
        #[serde(rename = "type")]
        binder_type: u32,
        body: u32,
        #[serde(rename = "binderInfo")]
        binder_info: BinderStyle,
    },
    #[serde(rename = "lam")]
    ExprLambda {
        #[serde(rename = "name")]
        binder_name: u32,
        #[serde(rename = "type")]
        binder_type: u32,
        body: u32,
        #[serde(rename = "binderInfo")]
        binder_info: BinderStyle,
    },
    #[serde(rename = "proj")]
    ExprProj {
        #[serde(rename = "typeName")]
        type_name: u32,
        idx: usize,
        #[serde(rename = "struct")]
        structure: u32,
    },
    #[serde(rename = "sort")]
    ExprSort(u32),
    #[serde(rename = "bvar")]
    ExprBVar(u16),
    #[serde(rename = "axiom")]
    Axiom {
        name: u32,
        #[serde(rename = "levelParams")]
        uparams: Vec<u32>,
        #[serde(rename = "type")]
        ty: u32,
        #[serde(rename = "isUnsafe")]
        is_unsafe: bool,
    },
    #[serde(rename = "thm")]
    Thm {
        name: u32,
        #[serde(rename = "levelParams")]
        uparams: Vec<u32>,
        #[serde(rename = "type")]
        ty: u32,
        value: u32,
    },
    #[serde(rename = "def")]
    Defn {
        name: u32,
        #[serde(rename = "levelParams")]
        uparams: Vec<u32>,
        #[serde(rename = "type")]
        ty: u32,
        value: u32,
        #[serde(rename = "hints")]
        hint: ReducibilityHint,
        //all: Vec<usize>,
        safety: DefinitionSafety,
    },
    #[serde(rename = "opaque")]
    Opaque {
        name: u32,
        #[serde(rename = "levelParams")]
        uparams: Vec<u32>,
        #[serde(rename = "type")]
        ty: u32,
        value: u32,
        #[serde(rename = "isUnsafe")]
        is_unsafe: bool,
    },
    #[serde(rename = "quot")]
    Quot {
        name: u32,
        #[serde(rename = "levelParams")]
        uparams: Vec<u32>,
        #[serde(rename = "type")]
        ty: u32,
        #[serde(rename = "kind")]
        kind: QuotKind,
    },
    #[serde(rename = "inductive")]
    Inductive {
        #[serde(rename = "types")]
        ind_vals: Vec<IndInfo>,
        #[serde(rename = "ctors")]
        ctor_vals: Vec<Constructor>,
        #[serde(rename = "recs")]
        rec_vals: Vec<Recursor>,
    },
}

/// [lean-zkvm patch] JSON の解析だけを行い、DAG への取り込みを飛ばす。
///
/// `parse_export_file` のコストが「JSON の解析」と「DAG への取り込み」の
/// どちらに寄っているかを切り分けるための計測用。返り値は解析した行数。
pub fn parse_json_only<R: BufRead>(mut buf_reader: R) -> Result<usize, Box<dyn Error>> {
    let mut line_buffer = String::new();
    let mut n = 0usize;
    loop {
        let amt = buf_reader.read_line(&mut line_buffer)?;
        if amt == 0 {
            break;
        }
        let _obj = serde_json::from_str::<ExportJsonObject>(line_buffer.as_str())?;
        n += 1;
        line_buffer.clear();
    }
    Ok(n)
}

pub fn parse_export_file<'p, R: BufRead>(
    buf_reader: R,
    config: Config,
) -> Result<(crate::util::ExportFile<'p>, Vec<String>), Box<dyn Error>> {
    let mut parser = Parser::new(buf_reader, config);
    let mut line_buffer = String::new();

    loop {
        let amt = parser.buf_reader.read_line(&mut line_buffer)?;
        if amt == 0 {
            break;
        }
        parser.go1(line_buffer.as_str())?;
        parser.line_num += 1;
        line_buffer.clear();
    }

    finish(parser)
}

/// [lean-zkvm patch] 取り込みが終わったパーサから `ExportFile` を組み立てる。
/// JSON 経路とバイナリ経路の共通部分。
fn finish<'p, R: BufRead>(parser: Parser<'p, R>) -> Result<(crate::util::ExportFile<'p>, Vec<String>), Box<dyn Error>> {
    // If the execution config has `unknown_pp_declar_hard_error: true`, and a `pp_declars`
    // that includes `foo`, then we return early with an error if no `foo` declaration is present
    // in the export file.
    if parser.config.unknown_pp_declar_hard_error {
        if let Some(pp_declars) = parser.config.pp_declars.as_ref() {
            let mut pp_declar_names = pp_declars.iter().map(|s| s.as_str()).collect::<crate::util::FxHashSet<&str>>();
            for declar_name in parser.declars.keys() {
                let n = parser.name_to_string(*declar_name);
                pp_declar_names.remove(n.as_str());
            }
            if pp_declar_names.len() > 0 {
                let list = pp_declar_names.into_iter().collect::<Vec<&str>>();
                return Err(Box::from(format!(
                    "these pp_declars were not found in the exported environment: {:#?}",
                    list
                )));
            }
        }
    }

    let name_cache = parser.dag.mk_name_cache();
    // Maps inductive names to exported recursor names. This is later reused in the inductive
    // module to require that the set of derived recursors matches the set of exported recursors,
    // so that additional "unassociated" recursors cannot be added to the environment.
    let mut ind_name_to_recursor_names = new_fx_hash_map();
    for declar in parser.declars.values() {
        match declar {
            Declar::Constructor(ConstructorData { inductive_name, info, .. }) => {
                match parser.declars.get(inductive_name).unwrap() {
                    Declar::Inductive(InductiveData { all_ctor_names, .. }) => {
                        assert!(all_ctor_names.contains(&info.name))
                    }
                    _ => panic!("failed to find inductive {:?}", parser.name_to_string(*inductive_name)),
                }
            }
            Declar::Recursor(RecursorData { all_inductives, info, .. }) => {
                for ind_name in all_inductives.iter().copied() {
                    ind_name_to_recursor_names.entry(ind_name).or_insert(new_fx_hash_set()).insert(info.name);
                }
            }
            _ => continue,
        }
    }

    let export_file = crate::util::ExportFile {
        dag: parser.dag,
        declars: parser.declars,
        notations: parser.notations,
        name_cache,
        config: parser.config,
        mutual_block_sizes: parser.mutual_block_sizes,
        ind_name_to_recursor_names,
    };
    Ok((export_file, parser.skipped))
}

impl<'a, R: BufRead> Parser<'a, R> {
    pub fn new(buf_reader: R, config: Config) -> Self {
        Self {
            buf_reader,
            line_num: 0usize,
            dag: LeanDag::new(&config),
            declars: new_fx_index_map(),
            notations: new_fx_hash_map(),
            config,
            skipped: Vec::new(),
            mutual_block_sizes: new_fx_hash_map(),
        }
    }

    fn axiom_permitted(&self, n: NamePtr<'a>) -> bool {
        self.config.unsafe_permit_all_axioms
            || self.config.permitted_axioms.as_ref().map(|v| v.contains(&self.name_to_string(n))).unwrap_or(false)
    }

    fn num_loose_bvars(&self, e: ExprPtr<'a>) -> u16 {
        self.dag.exprs.get_index(e.idx()).unwrap().num_loose_bvars()
    }

    fn has_fvars(&self, e: ExprPtr<'a>) -> bool {
        self.dag.exprs.get_index(e.idx()).unwrap().has_fvars()
    }

    fn get_name_ptr(&self, idx: u32) -> NamePtr<'a> {
        let out = crate::util::Ptr::from(DagMarker::ExportFile, idx as usize);
        assert!((idx as usize) < self.dag.names.len());
        out
    }

    fn get_level_ptr(&self, idx: u32) -> LevelPtr<'a> {
        let out = crate::util::Ptr::from(DagMarker::ExportFile, idx as usize);
        assert!((idx as usize) < self.dag.levels.len());
        out
    }
    fn get_names(&self, idxs: &[u32]) -> Vec<NamePtr<'a>> {
        let mut names = Vec::new();
        for idx in idxs.iter().copied() {
            assert!(self.dag.names.get_index(idx as usize).is_some());
            names.push(NamePtr::from(DagMarker::ExportFile, idx as usize));
        }
        names
    }

    fn get_uparams_ptr(&mut self, name_idxs: &[u32]) -> LevelsPtr<'a> {
        let mut levels = Vec::new();
        for name_idx in name_idxs.iter().copied() {
            let name_ptr = self.get_name_ptr(name_idx);
            let hash = hash64!(crate::level::PARAM_HASH, name_ptr);
            // Has to already exist
            let idx = self.dag.levels.get_index_of(&Level::Param(name_ptr, hash)).unwrap();
            levels.push(LevelPtr::from(DagMarker::ExportFile, idx as usize));
        }
        LevelsPtr::from(DagMarker::ExportFile, self.dag.uparams.insert_full(Arc::from(levels)).0)
    }

    fn get_levels_ptr(&mut self, idxs: &[u32]) -> LevelsPtr<'a> {
        let mut levels = Vec::new();
        for idx in idxs.iter().copied() {
            levels.push(LevelPtr::from(DagMarker::ExportFile, idx as usize));
        }
        LevelsPtr::from(DagMarker::ExportFile, self.dag.uparams.insert_full(Arc::from(levels)).0)
    }

    fn get_expr_ptr(&self, idx: u32) -> ExprPtr<'a> {
        let out = crate::util::Ptr::from(DagMarker::ExportFile, idx as usize);
        assert!((idx as usize) < self.dag.exprs.len());
        out
    }

    // Used for the axiom whitelist feature.
    fn name_to_string(&self, n: NamePtr<'a>) -> String {
        match self.dag.names.get_index(n.idx()).copied().unwrap() {
            Name::Anon => String::new(),
            Name::Str(pfx, sfx, _) => {
                let mut s = self.name_to_string(pfx);
                if !s.is_empty() {
                    s.push('.');
                }
                s + self.dag.strings.get_index(sfx.idx()).unwrap()
            }
            Name::Num(pfx, sfx, _) => {
                let mut s = self.name_to_string(pfx);
                if !s.is_empty() {
                    s.push('.');
                }
                s + format!("{}", sfx).as_str()
            }
        }
    }

    fn go1(&mut self, line: &str) -> Result<(), Box<dyn Error>> {
        let ExportJsonObject { val, i: assigned_idx } = serde_json::from_str::<ExportJsonObject>(line)?;
        self.apply_record(val, assigned_idx)
    }

    /// [lean-zkvm patch] 刈り取られたノードの位置に、一意で「使えない」値を置く。
    ///
    /// 番号を保つためだけのもの。`hash` フィールドに通し番号を入れているので
    /// 必ず一意になり、重複排除で潰れない。Expr は不正な deBruijn 番号を持つので、
    /// 万一 kernel が読めば失敗する。
    pub(crate) fn insert_placeholder(&mut self, kind: u8) {
        let uniq = self.line_num as u64 | (1u64 << 63);
        match kind {
            0 => {
                let anon = self.dag.anonymous();
                self.dag.names.insert_full(crate::name::Name::Num(anon, uniq, uniq));
            }
            1 => {
                let anon = self.dag.anonymous();
                self.dag.levels.insert_full(crate::level::Level::Param(anon, uniq));
            }
            _ => {
                self.dag.exprs.insert_full(crate::expr::Expr::Var { hash: uniq, dbj_idx: u16::MAX });
            }
        };
    }

    /// [lean-zkvm patch] 解析済みのレコードを DAG に取り込む。
    /// JSON 経路とバイナリ経路の共通部分。
    pub(crate) fn apply_record<'x>(
        &mut self,
        val: ExportJsonVal<'x>,
        assigned_idx: Option<BackRef>,
    ) -> Result<(), Box<dyn Error>> {
        use ExportJsonVal::*;
        match val {
            Metadata(json_val) => {
                let _ = check_semver(&json_val)?;
            }
            NameStr { pre, str } => {
                let pfx = self.get_name_ptr(pre);
                let sfx = StringPtr::from(
                    DagMarker::ExportFile,
                    self.dag.strings.insert_full(std::borrow::Cow::Owned(str.to_string())).0,
                );

                let insert_result = {
                    let hash = hash64!(crate::name::STR_HASH, pfx, sfx);
                    self.dag.names.insert_full(Name::Str(pfx, sfx, hash))
                };
                assigned_idx.unwrap().assert_in(insert_result);
            }
            NameNum { pre, i } => {
                let pfx = self.get_name_ptr(pre);
                let sfx = i as u64;
                let insert_result = {
                    let hash = hash64!(crate::name::NUM_HASH, pfx, sfx);
                    self.dag.names.insert_full(Name::Num(pfx, sfx, hash))
                };
                assigned_idx.unwrap().assert_in(insert_result);
            }
            NatLit(big_uint) => {
                if !self.config.nat_extension {
                    return Err(Box::<dyn Error>::from(
                        format!("Nat lit extension disallowed by checker execution config, but export file contains a nat literal (line {})", self.line_num)
                    ));
                }
                let num_ptr =
                    BigUintPtr::from(DagMarker::ExportFile, self.dag.bignums.as_mut().unwrap().insert_full(big_uint).0);
                let insert_result = {
                    let hash = hash64!(crate::expr::NAT_LIT_HASH, num_ptr);
                    self.dag.exprs.insert_full(Expr::NatLit { ptr: num_ptr, hash })
                };
                if !self.config.nat_extension {
                    return Err(Box::<dyn Error>::from(format!(
                        "Nat lit extension disallowed by checker execution config, found (line {})",
                        self.line_num
                    )));
                }
                assigned_idx.unwrap().assert_ie(insert_result);
            }
            StrLit(cow_str) => {
                if !self.config.string_extension {
                    return Err(Box::<dyn Error>::from(
                        format!("String lit extension disallowed by checker execution config, but export file contains a string literal (line {})", self.line_num)
                    ));
                }
                let s = cow_str.to_string();
                let string_ptr = StringPtr::from(
                    DagMarker::ExportFile,
                    self.dag.strings.insert_full(crate::util::CowStr::Owned(s)).0,
                );
                let insert_result = {
                    let hash = hash64!(crate::expr::STRING_LIT_HASH, string_ptr);
                    self.dag.exprs.insert_full(Expr::StringLit { ptr: string_ptr, hash })
                };
                assigned_idx.unwrap().assert_ie(insert_result);
            }
            LevelSucc(l) => {
                let l = self.get_level_ptr(l);
                let insert_result = {
                    let hash = hash64!(crate::level::SUCC_HASH, l);
                    self.dag.levels.insert_full(Level::Succ(l, hash))
                };
                assigned_idx.unwrap().assert_il(insert_result);
            }
            LevelMax([l, r]) => {
                let l = self.get_level_ptr(l);
                let r = self.get_level_ptr(r);
                let insert_result = {
                    let hash = hash64!(crate::level::MAX_HASH, l, r);
                    self.dag.levels.insert_full(Level::Max(l, r, hash))
                };
                assigned_idx.unwrap().assert_il(insert_result);
            }
            LevelIMax([l, r]) => {
                let l = self.get_level_ptr(l);
                let r = self.get_level_ptr(r);
                let insert_result = {
                    let hash = hash64!(crate::level::IMAX_HASH, l, r);
                    self.dag.levels.insert_full(Level::IMax(l, r, hash))
                };
                assigned_idx.unwrap().assert_il(insert_result);
            }
            LevelParam(var_idx) => {
                let n = self.get_name_ptr(var_idx);
                let insert_result = {
                    let hash = hash64!(crate::level::PARAM_HASH, n);
                    self.dag.levels.insert_full(Level::Param(n, hash))
                };
                assigned_idx.unwrap().assert_il(insert_result);
            }
            ExprSort(level) => {
                let level = self.get_level_ptr(level);
                let insert_result = {
                    let hash = hash64!(crate::expr::SORT_HASH, level);
                    self.dag.exprs.insert_full(Expr::Sort { level, hash })
                };
                assigned_idx.unwrap().assert_ie(insert_result);
            }
            ExprMData { .. } => {
                panic!("Expr.mdata not supported");
            }
            ExprConst { name, levels } => {
                let name = self.get_name_ptr(name);
                let levels = self.get_levels_ptr(&levels);
                let insert_result = {
                    let hash = hash64!(crate::expr::CONST_HASH, name, levels);
                    self.dag.exprs.insert_full(Expr::Const { name, levels, hash })
                };
                assigned_idx.unwrap().assert_ie(insert_result);
            }
            ExprApp { fun, arg } => {
                let fun = self.get_expr_ptr(fun);
                let arg = self.get_expr_ptr(arg);
                let insert_result = {
                    let hash = hash64!(crate::expr::APP_HASH, fun, arg);
                    let num_bvars = self.num_loose_bvars(fun).max(self.num_loose_bvars(arg));
                    let locals = self.has_fvars(fun) || self.has_fvars(arg);
                    self.dag.exprs.insert_full(Expr::App {
                        fun,
                        arg,
                        num_loose_bvars: num_bvars,
                        has_fvars: locals,
                        hash,
                    })
                };
                assigned_idx.unwrap().assert_ie(insert_result);
            }
            ExprBVar(dbj_idx) => {
                let insert_result = {
                    let hash = hash64!(crate::expr::VAR_HASH, dbj_idx);
                    self.dag.exprs.insert_full(Expr::Var { dbj_idx, hash })
                };
                assigned_idx.unwrap().assert_ie(insert_result);
            }
            ExprLambda { binder_name, binder_type, binder_info, body } => {
                let binder_name = self.get_name_ptr(binder_name);
                let binder_type = self.get_expr_ptr(binder_type);
                let body = self.get_expr_ptr(body);
                let insert_result = {
                    let hash = hash64!(crate::expr::LAMBDA_HASH, binder_name, binder_info, binder_type, body);
                    let num_bvars = self.num_loose_bvars(binder_type).max(self.num_loose_bvars(body).saturating_sub(1));
                    let locals = self.has_fvars(binder_type) || self.has_fvars(body);
                    self.dag.exprs.insert_full(Expr::Lambda {
                        binder_name,
                        binder_style: binder_info,
                        binder_type,
                        body,
                        num_loose_bvars: num_bvars,
                        has_fvars: locals,
                        hash,
                    })
                };
                assigned_idx.unwrap().assert_ie(insert_result);
            }
            ExprPi { binder_name, binder_type, binder_info, body } => {
                let binder_name = self.get_name_ptr(binder_name);
                let binder_type = self.get_expr_ptr(binder_type);
                let body = self.get_expr_ptr(body);
                let insert_result = {
                    let hash = hash64!(crate::expr::PI_HASH, binder_name, binder_info, binder_type, body);
                    let num_bvars = self.num_loose_bvars(binder_type).max(self.num_loose_bvars(body).saturating_sub(1));
                    let locals = self.has_fvars(binder_type) || self.has_fvars(body);
                    self.dag.exprs.insert_full(Expr::Pi {
                        binder_name,
                        binder_style: binder_info,
                        binder_type,
                        body,
                        num_loose_bvars: num_bvars,
                        has_fvars: locals,
                        hash,
                    })
                };
                assigned_idx.unwrap().assert_ie(insert_result);
            }
            ExprLet { name, ty, value, body, nondep } => {
                let binder_name = self.get_name_ptr(name);
                let binder_type = self.get_expr_ptr(ty);
                let val = self.get_expr_ptr(value);
                let body = self.get_expr_ptr(body);
                let insert_result = {
                    let hash = hash64!(crate::expr::LET_HASH, binder_name, binder_type, val, body, nondep);
                    let num_bvars = self
                        .num_loose_bvars(binder_type)
                        .max(self.num_loose_bvars(val).max(self.num_loose_bvars(body).saturating_sub(1)));
                    let locals = self.has_fvars(binder_type) || self.has_fvars(val) || self.has_fvars(body);
                    self.dag.exprs.insert_full(Expr::Let {
                        binder_name,
                        binder_type,
                        val,
                        body,
                        num_loose_bvars: num_bvars,
                        has_fvars: locals,
                        hash,
                        nondep,
                    })
                };
                assigned_idx.unwrap().assert_ie(insert_result);
            }
            ExprProj { type_name, idx, structure: struct_ } => {
                let ty_name = self.get_name_ptr(type_name);
                let structure = self.get_expr_ptr(struct_);
                let insert_result = {
                    let hash = hash64!(crate::expr::PROJ_HASH, ty_name, idx, structure);
                    let num_bvars = self.num_loose_bvars(structure);
                    let locals = self.has_fvars(structure);
                    self.dag.exprs.insert_full(Expr::Proj {
                        ty_name,
                        idx,
                        structure,
                        num_loose_bvars: num_bvars,
                        has_fvars: locals,
                        hash,
                    })
                };
                assigned_idx.unwrap().assert_ie(insert_result);
            }
            Axiom { name, ty, uparams, is_unsafe } => {
                assert!(!is_unsafe);
                let name = self.get_name_ptr(name);
                let uparams = self.get_uparams_ptr(&uparams);
                let ty = self.get_expr_ptr(ty);
                let info = DeclarInfo { name, ty, uparams };
                let axiom = Declar::Axiom { info };
                if self.axiom_permitted(name) {
                    assert!(self.declars.insert(name, axiom).is_none());
                } else {
                    let name_string = self.name_to_string(name);
                    if self.config.unpermitted_axiom_hard_error {
                        return Err(Box::from(format!("export file declares unpermitted axiom {:?}", name_string)));
                    } else {
                        self.skipped.push(name_string)
                    }
                }
            }
            Defn { name, ty, uparams, value, hint, safety } => {
                assert!(!matches!(safety, DefinitionSafety::Unsafe | DefinitionSafety::Partial));
                let name = self.get_name_ptr(name);
                let ty = self.get_expr_ptr(ty);
                let val = self.get_expr_ptr(value);
                let uparams = self.get_uparams_ptr(&uparams);
                let info = DeclarInfo { name, ty, uparams };
                let definition = Declar::Definition { info, val, hint };
                assert!(self.declars.insert(name, definition).is_none());
            }
            Thm { name, ty, uparams, value } => {
                let name = self.get_name_ptr(name);
                let ty = self.get_expr_ptr(ty);
                let val = self.get_expr_ptr(value);
                let uparams = self.get_uparams_ptr(&uparams);
                let info = DeclarInfo { name, ty, uparams };
                let theorem = Declar::Theorem { info, val };
                assert!(self.declars.insert(name, theorem).is_none());
            }
            Opaque { name, ty, uparams, value, is_unsafe } => {
                assert!(!is_unsafe);
                let name = self.get_name_ptr(name);
                let ty = self.get_expr_ptr(ty);
                let val = self.get_expr_ptr(value);
                let uparams = self.get_uparams_ptr(&uparams);
                let info = DeclarInfo { name, ty, uparams };
                let definition = Declar::Opaque { info, val };
                assert!(self.declars.insert(name, definition).is_none());
            }
            Quot { name, ty, uparams, .. } => {
                let name = self.get_name_ptr(name);
                let ty = self.get_expr_ptr(ty);
                let uparams = self.get_uparams_ptr(&uparams);
                let info = DeclarInfo { name, ty, uparams };
                let quot = Declar::Quot { info };
                assert!(self.declars.insert(name, quot).is_none());
            }
            Inductive { ind_vals, ctor_vals, rec_vals } => {
                let block_start = self.declars.len();
                let block_size = ind_vals.len() + ctor_vals.len() + rec_vals.len();
                for IndInfo {
                    name,
                    ty,
                    uparams,
                    all,
                    ctors,
                    is_rec,
                    num_nested,
                    num_params,
                    num_indices,
                    is_unsafe,
                    ..
                } in ind_vals
                {
                    assert!(!is_unsafe);
                    let name = self.get_name_ptr(name);
                    self.mutual_block_sizes.insert(name, (block_start, block_size));
                    let uparams = self.get_uparams_ptr(&uparams);
                    let ty = self.get_expr_ptr(ty);
                    let all_ind_names = Arc::from(self.get_names(&all));
                    let all_ctor_names = Arc::from(self.get_names(&ctors));
                    let inductive = Declar::Inductive(InductiveData {
                        info: DeclarInfo { name, uparams, ty },
                        is_recursive: is_rec,
                        is_nested: num_nested > 0,
                        num_params,
                        num_indices,
                        all_ind_names,
                        all_ctor_names,
                    });
                    assert!(self.declars.insert(name, inductive).is_none());
                }
                for Constructor { name, uparams, ty, is_unsafe, induct, cidx, num_params, num_fields, .. } in ctor_vals
                {
                    assert!(!is_unsafe);
                    let name = self.get_name_ptr(name);
                    let ty = self.get_expr_ptr(ty);
                    let uparams = self.get_uparams_ptr(&uparams);
                    let info = DeclarInfo { name, ty, uparams };
                    let parent_inductive = self.get_name_ptr(induct);
                    let ctor_idx = cidx;
                    let ctor = Declar::Constructor(ConstructorData {
                        info,
                        inductive_name: parent_inductive,
                        ctor_idx,
                        num_params,
                        num_fields,
                    });
                    assert!(self.declars.insert(name, ctor).is_none());
                }
                for Recursor {
                    name,
                    uparams,
                    ty,
                    rules,
                    is_unsafe,
                    num_params,
                    num_indices,
                    num_motives,
                    num_minors,
                    k,
                    all,
                    ..
                } in rec_vals
                {
                    assert!(!is_unsafe);
                    let name = self.get_name_ptr(name);
                    let ty = self.get_expr_ptr(ty);
                    let uparams = self.get_uparams_ptr(&uparams);
                    let info = DeclarInfo { name, ty, uparams };
                    let rules = rules
                        .into_iter()
                        .map(|RecursorRule { rhs, ctor, nfields }| crate::env::RecRule {
                            val: self.get_expr_ptr(rhs),
                            ctor_name: self.get_name_ptr(ctor),
                            ctor_telescope_size_wo_params: nfields,
                        })
                        .collect::<Vec<_>>();
                    let all_inductives = self.get_names(&all);
                    let recursor = Declar::Recursor(RecursorData {
                        info,
                        all_inductives: Arc::from(all_inductives),
                        num_params,
                        num_indices,
                        num_motives,
                        num_minors,
                        rec_rules: Arc::from(rules),
                        is_k: k,
                    });
                    assert!(self.declars.insert(name, recursor).is_none())
                }
            }
        }
        Ok(())
    }
}

/// [lean-zkvm patch] `deserialize_biguint_from_string` と対になる書き出し。
/// 読み側が文字列を期待しているので、書き側も文字列にしないと往復しない。
fn serialize_biguint_as_string<S>(n: &BigUint, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_str(&n.to_string())
}

/// Needed because the lean4export format serializes nat literals as strings:
/// https://github.com/leanprover/lean4export/blob/ddeb0869b0b5679b0104e16291ffd929fbaa6a48/format_ndjson.md?plain=1#L186
fn deserialize_biguint_from_string<'de, D>(deserializer: D) -> Result<BigUint, D::Error>
where
    D: Deserializer<'de>,
{
    use std::str::FromStr;
    struct BigUintStringVisitor;

    impl<'de> Visitor<'de> for BigUintStringVisitor {
        type Value = BigUint;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("a string containing a natural number")
        }

        fn visit_str<E>(self, v: &str) -> Result<BigUint, E>
        where
            E: DeError,
        {
            BigUint::from_str(v).map_err(|e| E::custom(format!("invalid BigUint decimal string: {e}")))
        }

        fn visit_string<E>(self, v: String) -> Result<BigUint, E>
        where
            E: DeError,
        {
            self.visit_str(&v)
        }
    }
    deserializer.deserialize_str(BigUintStringVisitor)
}

mod semver_tests {
    use super::*;
    #[allow(dead_code)]
    fn mk_meta(s: &'static str) -> FileMeta<'static> {
        FileMeta {
            lean: LeanMeta { version: Cow::Borrowed(""), githash: Cow::Borrowed("") },
            exporter: ExporterMeta { version: Cow::Borrowed(""), name: Cow::Borrowed("") },
            format: FormatMeta { version: Cow::Borrowed(s) },
        }
    }

    #[test]
    fn test_ng() {
        let too_small = ["2.9.9", "2.9.99"];
        let too_big = ["4.0.0", "4.1.0", "3.2.0", "3.2.1"];

        for v in too_small {
            assert!(check_semver(&mk_meta(v)).is_err())
        }
        for v in too_big {
            assert!(check_semver(&mk_meta(v)).is_err())
        }
    }

    #[test]
    fn test_ok() {
        let ok = ["3.1.0", "3.1.9"];
        for v in ok {
            assert!(check_semver(&mk_meta(v)).is_ok())
        }
    }
}

/// [lean-zkvm patch] flatten を使わないレコード形式。
///
/// export の JSON は「値のキー」と「割り当てるインデックスのキー」が同じ階層に
/// 並ぶため `#[serde(flatten)]` が要る。しかし flatten はオブジェクトを一度
/// 中間表現にバッファしてから再デシリアライズするので非常に高価で、実測では
/// export の解析コストの 9 割を占めていた。
///
/// 自分で決められるバイナリ形式なら flatten は不要なので、素直な struct にする。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct ExportRecord<'a> {
    pub val: ExportJsonVal<'a>,
    pub i: Option<BackRef>,
}

/// NDJSON をバイナリ形式（長さ前置の postcard レコード列）に変換する。
/// zkVM の外で走らせる前提。決定的なので、検証者も同じ変換をして
/// 同じバイト列・同じハッシュを得られる。
pub fn ndjson_to_binary<R: BufRead>(mut reader: R) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut out = Vec::new();
    let mut line = String::new();
    loop {
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        let ExportJsonObject { val, i } = serde_json::from_str::<ExportJsonObject>(line.as_str())?;
        let bytes = postcard::to_allocvec(&ExportRecord { val, i })?;
        out.extend_from_slice(&u32::try_from(bytes.len())?.to_le_bytes());
        out.extend_from_slice(&bytes);
        line.clear();
    }
    Ok(out)
}

/// [lean-zkvm patch] 読まれないノードをプレースホルダに置き換えたバイナリを作る。
///
/// 番号（back-reference）は詰めずにそのまま保つので、参照の付け替えが要らない。
/// 置き換えたノードが実は必要だった場合、kernel はプレースホルダを読んで
/// 失敗する。つまり **型検査が通ること自体が「その集合で自己完結している」ことの
/// 証拠**になる。
///
/// `keep` は (種別, インデックス) の集合。種別は 0=Name, 1=Level, 2=Expr。
///
/// ただし刈り取るのは Expr だけ。Name と Level は宣言そのものが参照する
/// （宣言名、universe パラメータ）ので、パース時点で必要になり残さざるを得ない。
/// 幸い数が少なく、この export では 6,950 レコード中 1,027 しかない。
pub fn prune_binary(
    blob: &[u8],
    keep: &std::collections::HashSet<(u8, u32)>,
) -> Result<(Vec<u8>, usize, usize), Box<dyn Error>> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    let (mut kept, mut pruned) = (0usize, 0usize);
    while pos < blob.len() {
        let len_bytes: [u8; 4] = blob.get(pos..pos + 4).ok_or("truncated record length")?.try_into()?;
        let len = u32::from_le_bytes(len_bytes) as usize;
        let rec_bytes = blob.get(pos + 4..pos + 4 + len).ok_or("truncated record")?;
        pos += 4 + len;

        let rec: ExportRecord = postcard::from_bytes(rec_bytes)?;
        // 番号を割り当てないレコード（メタデータと宣言）は必ず残す。
        let slot = match &rec.i {
            None => None,
            Some(BackRef::In(i)) => Some((0u8, *i)),
            Some(BackRef::Il(i)) => Some((1u8, *i)),
            Some(BackRef::Ie(i)) => Some((2u8, *i)),
        };
        match slot {
            // Expr だけを刈り取る（種別 2）
            Some(s) if s.0 == 2 && !keep.contains(&s) => {
                // プレースホルダ: 長さ 1、中身は種別のみ
                out.extend_from_slice(&1u32.to_le_bytes());
                out.push(s.0);
                pruned += 1;
            }
            _ => {
                out.extend_from_slice(&len_bytes);
                out.extend_from_slice(rec_bytes);
                kept += 1;
            }
        }
    }
    Ok((out, kept, pruned))
}

/// バイナリ形式の export を読み込む。`parse_export_file` と同じ結果になる。
pub fn parse_export_file_binary<'p>(
    blob: &[u8],
    config: Config,
) -> Result<(crate::util::ExportFile<'p>, Vec<String>), Box<dyn Error>> {
    let mut parser = Parser::new(std::io::Cursor::new(&[][..]), config);
    let mut pos = 0usize;
    while pos < blob.len() {
        let len_bytes: [u8; 4] = blob.get(pos..pos + 4).ok_or("truncated record length")?.try_into()?;
        let len = u32::from_le_bytes(len_bytes) as usize;
        pos += 4;
        let rec_bytes = blob.get(pos..pos + len).ok_or("truncated record")?;
        pos += len;
        if len == 1 {
            // prune_binary が置いたプレースホルダ。番号だけ進める。
            parser.insert_placeholder(rec_bytes[0]);
            parser.line_num += 1;
            continue;
        }
        let rec: ExportRecord = postcard::from_bytes(rec_bytes)?;
        parser.apply_record(rec.val, rec.i)?;
        parser.line_num += 1;
    }
    finish(parser)
}

#[derive(Debug, Clone, Copy)]
enum SparseDep {
    Node(u8, u32),
    UParam(u32),
}

fn parser_record_slices(blob: &[u8]) -> Result<Vec<&[u8]>, Box<dyn Error>> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos < blob.len() {
        let len = u32::from_le_bytes(blob.get(pos..pos + 4).ok_or("truncated record length")?.try_into()?) as usize;
        pos += 4;
        out.push(blob.get(pos..pos + len).ok_or("truncated record")?);
        pos += len;
    }
    Ok(out)
}

#[derive(Default)]
struct RecordMeta {
    slot: Option<(u8, u32)>,
    deps: Vec<SparseDep>,
    declar_names: Vec<u32>,
    is_metadata: bool,
}

fn add_uparam_dependencies(out: &mut RecordMeta, names: &[u32]) {
    for name in names {
        out.deps.push(SparseDep::Node(0, *name));
        out.deps.push(SparseDep::UParam(*name));
    }
}

fn record_meta(rec: &ExportRecord<'_>) -> RecordMeta {
    use ExportJsonVal::*;
    let mut out = RecordMeta {
        slot: rec.i.as_ref().map(|i| match i {
            BackRef::In(i) => (0, *i),
            BackRef::Il(i) => (1, *i),
            BackRef::Ie(i) => (2, *i),
        }),
        ..Default::default()
    };
    let node = |kind, idx| SparseDep::Node(kind, idx);
    match &rec.val {
        Metadata(_) => out.is_metadata = true,
        NameStr { pre, .. } | NameNum { pre, .. } => out.deps.push(node(0, *pre)),
        LevelSucc(l) => out.deps.push(node(1, *l)),
        LevelMax(ls) | LevelIMax(ls) => out.deps.extend(ls.iter().map(|i| node(1, *i))),
        LevelParam(name) => out.deps.push(node(0, *name)),
        NatLit(_) | StrLit(_) | ExprBVar(_) => {}
        ExprMData { expr, .. } => out.deps.push(node(2, *expr)),
        ExprLet { name, ty, value, body, .. } => {
            out.deps.push(node(0, *name));
            out.deps.extend([node(2, *ty), node(2, *value), node(2, *body)]);
        }
        ExprConst { name, levels } => {
            out.deps.push(node(0, *name));
            out.deps.extend(levels.iter().map(|i| node(1, *i)));
        }
        ExprApp { fun, arg } => out.deps.extend([node(2, *fun), node(2, *arg)]),
        ExprPi { binder_name, binder_type, body, .. }
        | ExprLambda { binder_name, binder_type, body, .. } => {
            out.deps.push(node(0, *binder_name));
            out.deps.extend([node(2, *binder_type), node(2, *body)]);
        }
        ExprProj { type_name, structure, .. } => {
            out.deps.push(node(0, *type_name));
            out.deps.push(node(2, *structure));
        }
        ExprSort(level) => out.deps.push(node(1, *level)),
        Axiom { name, uparams: us, ty, .. } | Quot { name, uparams: us, ty, .. } => {
            out.declar_names.push(*name);
            out.deps.extend([node(0, *name), node(2, *ty)]);
            add_uparam_dependencies(&mut out, us);
        }
        Thm { name, uparams: us, ty, value }
        | Opaque { name, uparams: us, ty, value, .. }
        | Defn { name, uparams: us, ty, value, .. } => {
            out.declar_names.push(*name);
            out.deps.extend([node(0, *name), node(2, *ty), node(2, *value)]);
            add_uparam_dependencies(&mut out, us);
        }
        Inductive { ind_vals, ctor_vals, rec_vals } => {
            for ind in ind_vals {
                out.declar_names.push(ind.name);
                out.deps.extend([node(0, ind.name), node(2, ind.ty)]);
                out.deps.extend(ind.all.iter().chain(&ind.ctors).map(|i| node(0, *i)));
                add_uparam_dependencies(&mut out, &ind.uparams);
            }
            for ctor in ctor_vals {
                out.declar_names.push(ctor.name);
                out.deps.extend([
                    node(0, ctor.name),
                    node(0, ctor.induct),
                    node(2, ctor.ty),
                ]);
                add_uparam_dependencies(&mut out, &ctor.uparams);
            }
            for recursor in rec_vals {
                out.declar_names.push(recursor.name);
                out.deps.extend([node(0, recursor.name), node(2, recursor.ty)]);
                out.deps.extend(recursor.all.iter().map(|i| node(0, *i)));
                for rule in &recursor.rules {
                    out.deps.extend([node(0, rule.ctor), node(2, rule.rhs)]);
                }
                add_uparam_dependencies(&mut out, &recursor.uparams);
            }
        }
    }
    out
}

/// Select the authenticated prelude records required to parse all suffix
/// records plus the named prelude declarations. The result contains original
/// record payloads and indices; no renumbering happens before Merkle verification.
pub fn select_sparse_prelude_records(
    prelude: &[u8],
    suffix: &[u8],
    declaration_name_indices: &std::collections::HashSet<u32>,
) -> Result<Vec<(u32, Vec<u8>)>, Box<dyn Error>> {
    let prelude_records = parser_record_slices(prelude)?;
    let suffix_records = parser_record_slices(suffix)?;
    let prelude_len = prelude_records.len();
    let all: Vec<&[u8]> = prelude_records.iter().chain(&suffix_records).copied().collect();

    let mut metas = Vec::with_capacity(all.len());
    let mut slots = std::collections::HashMap::new();
    let mut level_params = std::collections::HashMap::new();
    for (record_idx, bytes) in all.iter().enumerate() {
        let rec: ExportRecord = postcard::from_bytes(bytes)?;
        let meta = record_meta(&rec);
        if let Some(slot) = meta.slot {
            slots.insert(slot, record_idx);
        }
        if let ExportJsonVal::LevelParam(name) = rec.val {
            level_params.insert(name, record_idx);
        }
        metas.push(meta);
    }

    let mut selected = vec![false; all.len()];
    let mut work = std::collections::VecDeque::new();
    for (idx, meta) in metas.iter().enumerate() {
        let keep = idx >= prelude_len
            || meta.is_metadata
            || meta.declar_names.iter().any(|name| declaration_name_indices.contains(name));
        if keep {
            selected[idx] = true;
            work.push_back(idx);
        }
    }
    while let Some(idx) = work.pop_front() {
        for dep in &metas[idx].deps {
            let dependency = match dep {
                SparseDep::Node(0, 0) | SparseDep::Node(1, 0) => continue,
                SparseDep::Node(kind, old) => slots.get(&(*kind, *old)),
                SparseDep::UParam(name) => level_params.get(name),
            }
            .ok_or("selected record has a missing dependency")?;
            if !selected[*dependency] {
                selected[*dependency] = true;
                work.push_back(*dependency);
            }
        }
    }

    Ok(selected
        .into_iter()
        .take(prelude_len)
        .enumerate()
        .filter_map(|(idx, keep)| keep.then(|| (idx as u32, all[idx].to_vec())))
        .collect())
}

/// 旧 index から密な新 index への写像。
///
/// 二段階の圧縮では `HashMap` を使い、1 パス版では [`SparseIdxMap`] を使う。
/// 両方を同じ `remap_record` で使えるように抽象化する
/// （ジェネリクスなので単相化され、動的ディスパッチは無い）。
pub(crate) trait IdxMap {
    fn lookup(&self, key: u32) -> Option<u32>;
}

impl IdxMap for std::collections::HashMap<u32, u32> {
    fn lookup(&self, key: u32) -> Option<u32> { self.get(&key).copied() }
}

/// [lean-zkvm] 旧 index が疎にしか現れない場合の写像。
///
/// 旧 index の空間は **元の DAG 全体**（Mathlib 規模では 3,700 万レコード）だが、
/// sparse witness で実際に現れるのは選択された 1 万件程度だけ。これを
/// `Vec<Option<u32>>` で旧 index を添字にして引くと、使わない領域まで確保して
/// ゼロで埋めることになる。zkVM では **触ったページがそのまま cycle になる**ので、
/// 選択数ではなく環境の大きさにコストが比例してしまう（実測 parse 16.8M → 474.8M）。
///
/// `(旧, 新)` の対を旧 index 昇順で持ち、二分探索で引く。確保するのは実際に
/// 現れた分だけなので、コストは選択数に比例する。
///
/// 追加は昇順のことが多い（prelude は認証済みのファイル順、suffix はその続き）ので
/// 末尾追加を高速路にし、順序が乱れている場合だけ挿入位置を二分探索する。
/// 乱れた witness を作れるのは prover 自身だけで、損をするのも prover だけ。
pub(crate) struct SparseIdxMap {
    entries: Vec<(u32, u32)>,
}

impl SparseIdxMap {
    fn new(seed: &[(u32, u32)]) -> Self {
        Self { entries: seed.to_vec() }
    }

    /// 旧 index `old` に新 index `new` を割り当てる。既に割り当て済みなら `None`。
    fn assign(&mut self, old: u32, new: u32) -> Option<()> {
        match self.entries.last() {
            Some(&(last, _)) if old <= last => {
                match self.entries.binary_search_by_key(&old, |&(k, _)| k) {
                    Ok(_) => return None,
                    Err(pos) => self.entries.insert(pos, (old, new)),
                }
            }
            _ => self.entries.push((old, new)),
        }
        Some(())
    }
}

impl IdxMap for SparseIdxMap {
    fn lookup(&self, key: u32) -> Option<u32> {
        self.entries
            .binary_search_by_key(&key, |&(k, _)| k)
            .ok()
            .map(|i| self.entries[i].1)
    }
}

/// [lean-zkvm] 旧 index の散らばり方に応じて表を選ぶ。
///
/// 旧 index 空間が実際に現れる件数と同程度なら、添字 1 回で引ける密な表が速い。
/// 3 桁離れている（Mathlib 全体を prelude にすると 3,704 万 : 1.2 万）と、
/// 確保とページインだけでコストが支配されるので疎な表にする。
///
/// 実測（`MathlibDemo.thm_int` の Stage 2 の parse）:
///
/// | prelude | 密 | 疎 |
/// | --- | ---: | ---: |
/// | 1,265 宣言 / 53,288 record | **16.8M** | 20.8M |
/// | 213,144 宣言 / 37,049,179 record | 474.8M | **71.7M** |
pub(crate) enum IdxTable {
    Dense(Vec<Option<u32>>),
    Sparse(SparseIdxMap),
}

impl IdxTable {
    fn new(dense: bool, seed: &[(u32, u32)]) -> Self {
        if dense {
            let mut table = Vec::new();
            for &(old, new) in seed {
                if old as usize >= table.len() {
                    table.resize(old as usize + 1, None);
                }
                table[old as usize] = Some(new);
            }
            IdxTable::Dense(table)
        } else {
            IdxTable::Sparse(SparseIdxMap::new(seed))
        }
    }

    /// 旧 index `old` に新 index `new` を割り当てる。既に割り当て済みなら `None`。
    fn assign(&mut self, old: u32, new: u32) -> Option<()> {
        match self {
            IdxTable::Dense(table) => {
                let slot = old as usize;
                if slot >= table.len() {
                    table.resize(slot + 1, None);
                }
                if table[slot].is_some() {
                    return None;
                }
                table[slot] = Some(new);
                Some(())
            }
            IdxTable::Sparse(map) => map.assign(old, new),
        }
    }
}

impl IdxMap for IdxTable {
    fn lookup(&self, key: u32) -> Option<u32> {
        match self {
            IdxTable::Dense(table) => table.get(key as usize).copied().flatten(),
            IdxTable::Sparse(map) => map.lookup(key),
        }
    }
}

fn remap<M: IdxMap>(map: &M, old: &mut u32) -> Result<(), Box<dyn Error>> {
    *old = map.lookup(*old).ok_or("sparse record references an omitted node")?;
    Ok(())
}

fn remap_each<M: IdxMap>(xs: &mut [u32], map: &M) -> Result<(), Box<dyn Error>> {
    for x in xs {
        remap(map, x)?;
    }
    Ok(())
}

fn remap_record<M: IdxMap>(
    rec: &mut ExportRecord<'_>,
    names: &M,
    levels: &M,
    exprs: &M,
) -> Result<(), Box<dyn Error>> {
    use ExportJsonVal::*;
    let rn = |x: &mut u32| remap(names, x);
    let rl = |x: &mut u32| remap(levels, x);
    let re = |x: &mut u32| remap(exprs, x);
    match &mut rec.val {
        Metadata(_) | NatLit(_) | StrLit(_) | ExprBVar(_) => {}
        NameStr { pre, .. } | NameNum { pre, .. } => rn(pre)?,
        LevelSucc(l) => rl(l)?,
        LevelMax(ls) | LevelIMax(ls) => remap_each(ls, levels)?,
        LevelParam(name) => rn(name)?,
        ExprMData { expr, .. } => re(expr)?,
        ExprLet { name, ty, value, body, .. } => { rn(name)?; re(ty)?; re(value)?; re(body)?; }
        ExprConst { name, levels: ls } => { rn(name)?; remap_each(ls, levels)?; }
        ExprApp { fun, arg } => { re(fun)?; re(arg)?; }
        ExprPi { binder_name, binder_type, body, .. }
        | ExprLambda { binder_name, binder_type, body, .. } => {
            rn(binder_name)?; re(binder_type)?; re(body)?;
        }
        ExprProj { type_name, structure, .. } => { rn(type_name)?; re(structure)?; }
        ExprSort(level) => rl(level)?,
        Axiom { name, uparams, ty, .. } | Quot { name, uparams, ty, .. } => {
            rn(name)?; remap_each(uparams, names)?; re(ty)?;
        }
        Thm { name, uparams, ty, value }
        | Opaque { name, uparams, ty, value, .. }
        | Defn { name, uparams, ty, value, .. } => {
            rn(name)?; remap_each(uparams, names)?; re(ty)?; re(value)?;
        }
        Inductive { ind_vals, ctor_vals, rec_vals } => {
            for ind in ind_vals {
                rn(&mut ind.name)?; remap_each(&mut ind.uparams, names)?; re(&mut ind.ty)?;
                remap_each(&mut ind.all, names)?; remap_each(&mut ind.ctors, names)?;
            }
            for ctor in ctor_vals {
                rn(&mut ctor.name)?; remap_each(&mut ctor.uparams, names)?; re(&mut ctor.ty)?; rn(&mut ctor.induct)?;
            }
            for recursor in rec_vals {
                rn(&mut recursor.name)?; remap_each(&mut recursor.uparams, names)?; re(&mut recursor.ty)?;
                remap_each(&mut recursor.all, names)?;
                for rule in &mut recursor.rules { rn(&mut rule.ctor)?; re(&mut rule.rhs)?; }
            }
        }
    }
    rec.i = match rec.i.take() {
        Some(BackRef::In(mut i)) => { rn(&mut i)?; Some(BackRef::In(i)) }
        Some(BackRef::Il(mut i)) => { rl(&mut i)?; Some(BackRef::Il(i)) }
        Some(BackRef::Ie(mut i)) => { re(&mut i)?; Some(BackRef::Ie(i)) }
        None => None,
    };
    Ok(())
}

/// [lean-zkvm] 認証済み sparse witness を **1 パスで** 読み込む。
///
/// `compact_sparse_export` + `parse_export_file_binary` の二段構えは、同じ record を
/// 最大 3 回 deserialize していた（写像作り・書き換え・再 parse）。しかも中間の
/// バイト列を生成して捨てていた。ここでは record を一度だけ deserialize し、
/// 参照を密な index に張り替えて、そのまま arena に流し込む。
///
/// 健全性は二段階版と同じ:
///
/// - prelude record は **先に処理された prelude record しか参照できない**。
///   suffix の entry はこの時点でまだ表に無いので、認証済み record の欠落依存を
///   未認証の suffix で埋める攻撃は成立しない（二段階版が 2 つの写像を使い分けて
///   いたのと同じ効果が、処理順から自然に出る）
/// - 重複 back-reference は拒否する
/// - `prelude_records` は元の index で昇順かつ一意でなければならない
///
/// 返り値は `(export_file, skipped_axioms, prelude 側の宣言数)`。
/// 宣言境界をここで数えるので、呼び出し側が prefix を parse し直す必要もない。
pub fn parse_sparse_export<'p>(
    prelude_records: &[(u32, Vec<u8>)],
    suffix: &[u8],
    config: Config,
) -> Result<(crate::util::ExportFile<'p>, Vec<String>, usize), Box<dyn Error>> {
    if !prelude_records.windows(2).all(|w| w[0].0 < w[1].0) {
        return Err("sparse prelude indices must be sorted and unique".into());
    }
    let suffix_records = parser_record_slices(suffix)?;
    let mut parser = Parser::new(std::io::Cursor::new(&[][..]), config);

    // 旧 index -> 密な新 index。0 番は export 形式の暗黙の Anon / Zero。
    //
    // 旧 index 空間が実際に読む record 数と同程度なら密な表、桁違いに大きければ
    // 疎な表を使う（[`IdxTable`] 参照）。
    let max_old = prelude_records.last().map_or(0, |(i, _)| *i as usize);
    let dense = max_old <= (prelude_records.len() + suffix_records.len()).saturating_mul(8);
    let mut names = IdxTable::new(dense, &[(0, 0)]);
    let mut levels = IdxTable::new(dense, &[(0, 0)]);
    let mut exprs = IdxTable::new(dense, &[]);
    let (mut next_name, mut next_level, mut next_expr) = (1u32, 1u32, 0u32);

    fn assign(
        table: &mut IdxTable,
        old: u32,
        next: &mut u32,
        kind: &'static str,
    ) -> Result<u32, Box<dyn Error>> {
        let new = *next;
        table
            .assign(old, new)
            .ok_or_else(|| format!("duplicate {kind} back-reference"))?;
        *next = next.checked_add(1).ok_or_else(|| format!("{kind} index overflow"))?;
        Ok(new)
    }

    let mut num_prelude_declars = 0usize;
    let total = prelude_records.len() + suffix_records.len();
    for idx in 0..total {
        let bytes: &[u8] = if idx < prelude_records.len() {
            prelude_records[idx].1.as_slice()
        } else {
            suffix_records[idx - prelude_records.len()]
        };
        let mut rec: ExportRecord = postcard::from_bytes(bytes)?;

        // 自分の新 index を先に確定させる。record が自分自身を参照することは
        // ないので、参照の張り替えより先に入れて問題ない。
        match rec.i {
            Some(BackRef::In(old)) => {
                assign(&mut names, old, &mut next_name, "Name")?;
            }
            Some(BackRef::Il(old)) => {
                assign(&mut levels, old, &mut next_level, "Level")?;
            }
            Some(BackRef::Ie(old)) => {
                assign(&mut exprs, old, &mut next_expr, "Expr")?;
            }
            None => {}
        }
        remap_record(&mut rec, &names, &levels, &exprs)?;
        parser.apply_record(rec.val, rec.i)?;
        parser.line_num += 1;

        if idx + 1 == prelude_records.len() {
            num_prelude_declars = parser.declars.len();
        }
    }

    let skipped = parser.skipped.clone();
    let (export_file, skipped_axioms) = finish(parser)?;
    debug_assert_eq!(skipped, skipped_axioms);
    Ok((export_file, skipped_axioms, num_prelude_declars))
}

/// Verify-independent deterministic compaction. `prelude_records` must contain
/// original authenticated payloads sorted by original record index. References
/// are rewritten to dense indices, and omitted records are not materialized.
/// Returns `(compact_full_blob, compact_prelude_byte_len)`.
pub fn compact_sparse_export(
    prelude_records: &[(u32, Vec<u8>)],
    suffix: &[u8],
) -> Result<(Vec<u8>, usize), Box<dyn Error>> {
    if !prelude_records.windows(2).all(|w| w[0].0 < w[1].0) {
        return Err("sparse prelude indices must be sorted and unique".into());
    }
    let suffix_records = parser_record_slices(suffix)?;
    let all: Vec<&[u8]> = prelude_records
        .iter()
        .map(|(_, bytes)| bytes.as_slice())
        .chain(suffix_records.iter().copied())
        .collect();

    let mut names = std::collections::HashMap::from([(0u32, 0u32)]);
    let mut levels = std::collections::HashMap::from([(0u32, 0u32)]);
    let mut exprs = std::collections::HashMap::new();
    let (mut next_name, mut next_level, mut next_expr) = (1u32, 1u32, 0u32);
    let mut prelude_maps = None;
    for (idx, bytes) in all.iter().enumerate() {
        let rec: ExportRecord = postcard::from_bytes(bytes)?;
        match rec.i {
            Some(BackRef::In(old)) => {
                if names.insert(old, next_name).is_some() { return Err("duplicate Name back-reference".into()); }
                next_name = next_name.checked_add(1).ok_or("Name index overflow")?;
            }
            Some(BackRef::Il(old)) => {
                if levels.insert(old, next_level).is_some() { return Err("duplicate Level back-reference".into()); }
                next_level = next_level.checked_add(1).ok_or("Level index overflow")?;
            }
            Some(BackRef::Ie(old)) => {
                if exprs.insert(old, next_expr).is_some() { return Err("duplicate Expr back-reference".into()); }
                next_expr = next_expr.checked_add(1).ok_or("Expr index overflow")?;
            }
            None => {}
        }
        if idx + 1 == prelude_records.len() {
            prelude_maps = Some((names.clone(), levels.clone(), exprs.clone()));
        }
    }
    let (prelude_names, prelude_levels, prelude_exprs) = prelude_maps.unwrap_or_else(|| {
        (
            std::collections::HashMap::from([(0u32, 0u32)]),
            std::collections::HashMap::from([(0u32, 0u32)]),
            std::collections::HashMap::new(),
        )
    });

    let mut out = Vec::new();
    let mut prelude_bytes = 0usize;
    for (idx, bytes) in all.iter().enumerate() {
        let mut rec: ExportRecord = postcard::from_bytes(bytes)?;
        if idx < prelude_records.len() {
            // Authenticated prelude records may only refer to other authenticated
            // prelude records. An unauthenticated suffix cannot fill a missing node.
            remap_record(&mut rec, &prelude_names, &prelude_levels, &prelude_exprs)?;
        } else {
            remap_record(&mut rec, &names, &levels, &exprs)?;
        }
        let encoded = postcard::to_allocvec(&rec)?;
        out.extend_from_slice(&u32::try_from(encoded.len())?.to_le_bytes());
        out.extend_from_slice(&encoded);
        if idx + 1 == prelude_records.len() {
            prelude_bytes = out.len();
        }
    }
    Ok((out, prelude_bytes))
}
