//! Domain-separated Merkle commitment for length-prefixed export records.

use risc0_zkvm::sha::{Impl, Sha256};

pub type Hash = [u8; 32];

/// ハッシュ呼び出しの回数。どこにコストが乗っているかを切り分けるための計測用。
/// ホスト上で `verify` を走らせて読む（サイクル数は zkVM で測る）。
#[cfg(not(target_os = "zkvm"))]
pub mod counters {
    use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
    pub static LEAF: AtomicUsize = AtomicUsize::new(0);
    pub static NODE: AtomicUsize = AtomicUsize::new(0);
    pub static PAD: AtomicUsize = AtomicUsize::new(0);
    pub fn reset() {
        for c in [&LEAF, &NODE, &PAD] { c.store(0, Relaxed); }
    }
    pub fn read() -> (usize, usize, usize) {
        (LEAF.load(Relaxed), NODE.load(Relaxed), PAD.load(Relaxed))
    }
    pub(super) fn bump(c: &AtomicUsize) { c.fetch_add(1, Relaxed); }
}

#[cfg(not(target_os = "zkvm"))]
macro_rules! count { ($c:ident) => { counters::bump(&counters::$c) } }
#[cfg(target_os = "zkvm")]
macro_rules! count { ($c:ident) => { () } }

/// 木ごとのドメイン分離タグ。record 木と名前木の葉を取り違えられないようにする。
#[derive(Clone, Copy)]
pub struct Domain {
    leaf: u8,
    pad: u8,
}

/// binary record 列の木。タグは従来と同じ値なので root は変わらない。
pub const RECORD: Domain = Domain { leaf: 0, pad: 2 };
/// public 宣言名（ソート済み・一意）の木。
pub const NAME: Domain = Domain { leaf: 1, pad: 4 };

/// 木から開いた葉の集合と、その境界部分木ハッシュ。
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct SparseLeaves {
    /// `(leaf index, payload)` pairs, sorted by index.
    pub leaves: Vec<(u32, Vec<u8>)>,
    /// Boundary subtree hashes in deterministic depth-first order.
    pub proof: Vec<Hash>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SparsePrelude {
    /// `(original leaf index, original postcard payload)` pairs, sorted by index.
    pub records: Vec<(u32, Vec<u8>)>,
    /// Boundary subtree hashes in deterministic depth-first order.
    pub proof: Vec<Hash>,
    /// public 宣言名の木から開いた葉。suffix が定義する名前それぞれについて、
    /// **その名前が入る位置の左右の葉**を開く。これで「その名前は public 集合に
    /// 無い」ことを、集合全体を渡さずに示せる（非メンバーシップ証明）。
    ///
    /// 以前は集合全体（Mathlib 規模で 213,144 個）を witness に載せてハッシュして
    /// いた。それだけで 114.5M cycles かかり、環境の大きさに比例していた。
    pub names: SparseLeaves,
}

/// 名前集合を葉の並びに符号化する。ソート済みかつ一意でなければ `None`。
pub fn names_blob(names: &[String]) -> Option<Vec<u8>> {
    if !names.windows(2).all(|w| w[0] < w[1]) {
        return None;
    }
    let mut out = Vec::new();
    for name in names {
        out.extend_from_slice(&(name.len() as u32).to_le_bytes());
        out.extend_from_slice(name.as_bytes());
    }
    Some(out)
}

/// public 宣言名集合へのコミットメント。
pub fn names_root(names: &[String]) -> Option<(Hash, u32)> {
    let blob = names_blob(names)?;
    root_in(NAME, &blob).ok()
}

/// `queries` の非メンバーシップを示すのに必要な葉だけを開く。
///
/// 各 query について、ソート済み集合の中でその名前が入る位置の左右を開く
/// （端なら片側だけ）。開く葉は query 1 つにつき高々 2 枚。
pub fn prove_names(names: &[String], queries: &[String]) -> Option<SparseLeaves> {
    let blob = names_blob(names)?;
    let mut selected: Vec<u32> = Vec::new();
    for q in queries {
        let pos = names.partition_point(|n| n < q);
        if names.is_empty() {
            break;
        }
        if pos == 0 {
            selected.push(0);
        } else if pos == names.len() {
            selected.push((names.len() - 1) as u32);
        } else {
            selected.push((pos - 1) as u32);
            selected.push(pos as u32);
        }
    }
    selected.sort_unstable();
    selected.dedup();
    prove_in(NAME, &blob, &selected).ok()
}

pub fn verify_names(root: Hash, leaf_count: u32, opened: &SparseLeaves) -> bool {
    verify_in(NAME, root, leaf_count, &opened.leaves, &opened.proof)
}

/// 開いた葉から「`queries` のどれも public 集合に無い」ことを確かめる。
///
/// 呼ぶ前に [`verify_names`] を通しておくこと。ここでやるのは
/// 「開いた葉が query を挟んでいて、しかも**隣り合っている**」ことの確認だけ。
/// 隣接を見ないと、間に query 本人がいる可能性を排除できない。
///
/// コミットされた葉が名前順に並んでいることは前提にしている。root は検証者が
/// 自分の prelude から計算した値と journal で照合されるので、そこで担保される。
pub fn names_absent(opened: &SparseLeaves, leaf_count: u32, queries: &[String]) -> bool {
    if leaf_count == 0 {
        return true;
    }
    let mut names: Vec<(u32, &str)> = Vec::with_capacity(opened.leaves.len());
    for (idx, bytes) in &opened.leaves {
        match core::str::from_utf8(bytes) {
            Ok(name) => names.push((*idx, name)),
            Err(_) => return false,
        }
    }
    // index 昇順 = 名前昇順（コミットされた集合がソート済みなので）。
    if !names.windows(2).all(|w| w[0].0 < w[1].0 && w[0].1 < w[1].1) {
        return false;
    }
    queries.iter().all(|q| {
        let q = q.as_str();
        let pos = names.partition_point(|(_, n)| *n < q);
        if pos == 0 {
            // 開いたどの名前よりも小さい。最初の葉であることまで確かめる。
            names.first().is_some_and(|(i, n)| *i == 0 && q < *n)
        } else if pos == names.len() {
            names.last().is_some_and(|(i, n)| *i == leaf_count - 1 && q > *n)
        } else {
            let (li, ln) = names[pos - 1];
            let (ri, rn) = names[pos];
            ri == li + 1 && ln < q && q < rn
        }
    })
}

pub fn records(blob: &[u8]) -> Result<Vec<&[u8]>, &'static str> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos < blob.len() {
        let len_bytes: [u8; 4] = blob
            .get(pos..pos + 4)
            .ok_or("truncated record length")?
            .try_into()
            .map_err(|_| "bad record length")?;
        let len = u32::from_le_bytes(len_bytes) as usize;
        pos = pos.checked_add(4).ok_or("record offset overflow")?;
        let record = blob.get(pos..pos + len).ok_or("truncated record")?;
        pos = pos.checked_add(len).ok_or("record offset overflow")?;
        out.push(record);
    }
    Ok(out)
}

/// 内部ノード用の初期状態。`SHA-256("lean-zkvm/merkle/internal/v1")`。
///
/// 内部ノードはドメイン分離タグを前置する代わりに、この状態から圧縮関数を
/// 1 回かける。タグを付けると 1 + 32 + 32 = 65 byte になり、SHA のパディングで
/// 2 ブロックに膨らんでしまうため。葉は標準の初期状態＋タグ 0x00 で計算されるので、
/// 初期状態が違う内部ノードとは分離される。
const INTERNAL_IV: [u8; 32] = [
    0xe0, 0xd2, 0x57, 0x3b, 0x46, 0x9b, 0xa5, 0x13, 0xc4, 0x39, 0xa4, 0xf9, 0x03, 0x8d, 0xa8, 0xc4,
    0x2d, 0x7b, 0x4b, 0xef, 0xce, 0xf6, 0x91, 0xf7, 0x8e, 0xc9, 0xc4, 0x80, 0x68, 0x99, 0x92, 0x3d,
];

/// 可変長の入力をハッシュする。葉とパディングにのみ使う。
///
/// 短い入力ではヒープ確保が SHA 本体より高くつくので、スタックの固定長バッファに
/// 収まる場合はそちらを使う。
fn digest(parts: &[&[u8]]) -> Hash {
    let total: usize = parts.iter().map(|part| part.len()).sum();
    // レコードは平均 40 byte 程度。バッファはゼロ初期化されるので、
    // 大きすぎると初期化コストがハッシュ本体を上回る。
    const INLINE: usize = 96;
    if total <= INLINE {
        let mut buf = [0u8; INLINE];
        let mut at = 0;
        for part in parts {
            buf[at..at + part.len()].copy_from_slice(part);
            at += part.len();
        }
        return to_hash(Impl::hash_bytes(&buf[..total]));
    }
    let mut input = Vec::with_capacity(total);
    for part in parts {
        input.extend_from_slice(part);
    }
    to_hash(Impl::hash_bytes(&input))
}

fn to_hash(d: impl core::ops::Deref<Target = risc0_zkvm::sha::Digest>) -> Hash {
    d.as_bytes().try_into().expect("SHA-256 digest size")
}

fn as_digest(h: &Hash) -> risc0_zkvm::sha::Digest {
    risc0_zkvm::sha::Digest::try_from(h.as_slice()).expect("SHA-256 digest size")
}

/// 葉のハッシュを `Digest` のまま返す。
///
/// 汎用の `digest(&[&[u8]])` を経由すると、`total` の集計と 4 回のスライスコピーで
/// SHA 本体 (398 cycles) を上回るオーバーヘッド (約 494 cycles) がついていた。
/// ヘッダは固定長なので直接組む。
fn leaf_digest(dom: Domain, index: u32, bytes: &[u8]) -> risc0_zkvm::sha::Digest {
    count!(LEAF);
    const HDR: usize = 1 + 4 + 8;
    const INLINE: usize = 128;
    let total = HDR + bytes.len();
    if total <= INLINE {
        let mut buf = [0u8; INLINE];
        buf[0] = dom.leaf;
        buf[1..5].copy_from_slice(&index.to_le_bytes());
        buf[5..HDR].copy_from_slice(&(bytes.len() as u64).to_le_bytes());
        buf[HDR..total].copy_from_slice(bytes);
        return *Impl::hash_bytes(&buf[..total]);
    }
    let mut buf = Vec::with_capacity(total);
    buf.push(dom.leaf);
    buf.extend_from_slice(&index.to_le_bytes());
    buf.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
    buf.extend_from_slice(bytes);
    *Impl::hash_bytes(&buf)
}

fn leaf_hash(dom: Domain, index: u32, bytes: &[u8]) -> Hash {
    to_hash_d(&leaf_digest(dom, index, bytes))
}

fn padding_hash(dom: Domain, index: u32) -> Hash {
    count!(PAD);
    digest(&[&[dom.pad], &index.to_le_bytes()])
}

/// 内部ノード。ちょうど 64 byte なので圧縮関数 1 回で済む。
///
/// `Digest` のまま受け渡す。`[u8;32]` との相互変換は 1 回 75 cycles かかり、
/// ノードあたり 4 回通ると圧縮本体 (165 cycles) を上回ってしまう。
fn node_digest(iv: &risc0_zkvm::sha::Digest, left: &risc0_zkvm::sha::Digest, right: &risc0_zkvm::sha::Digest) -> risc0_zkvm::sha::Digest {
    count!(NODE);
    *Impl::compress(iv, left, right)
}

fn internal_iv() -> risc0_zkvm::sha::Digest {
    as_digest(&INTERNAL_IV)
}

fn to_hash_d(d: &risc0_zkvm::sha::Digest) -> Hash {
    d.as_bytes().try_into().expect("SHA-256 digest size")
}

fn node_hash(left: &Hash, right: &Hash) -> Hash {
    to_hash_d(&node_digest(&internal_iv(), &as_digest(left), &as_digest(right)))
}

fn tree(dom: Domain, blob: &[u8]) -> Result<(u32, usize, Vec<Hash>), &'static str> {
    let records = records(blob)?;
    let count = u32::try_from(records.len()).map_err(|_| "too many records")?;
    let width = records.len().max(1).next_power_of_two();
    let mut nodes = vec![[0; 32]; width * 2];
    for i in 0..width {
        nodes[width + i] = if let Some(bytes) = records.get(i) {
            leaf_hash(dom, i as u32, bytes)
        } else {
            padding_hash(dom, i as u32)
        };
    }
    for i in (1..width).rev() {
        nodes[i] = node_hash(&nodes[i * 2], &nodes[i * 2 + 1]);
    }
    Ok((count, width, nodes))
}

fn root_in(dom: Domain, blob: &[u8]) -> Result<(Hash, u32), &'static str> {
    let (count, _, nodes) = tree(dom, blob)?;
    Ok((nodes[1], count))
}

pub fn root(blob: &[u8]) -> Result<(Hash, u32), &'static str> {
    root_in(RECORD, blob)
}

fn prove_in(dom: Domain, blob: &[u8], selected: &[u32]) -> Result<SparseLeaves, &'static str> {
    let payloads = records(blob)?;
    let (count, width, nodes) = tree(dom, blob)?;
    if !selected.windows(2).all(|w| w[0] < w[1]) {
        return Err("selected record indices must be sorted and unique");
    }
    if selected.iter().any(|i| *i >= count) {
        return Err("selected record index out of range");
    }

    fn emit(
        node: usize,
        start: usize,
        size: usize,
        selected: &[u32],
        nodes: &[Hash],
        out: &mut Vec<Hash>,
    ) {
        let end = start + size;
        let first = selected.partition_point(|i| (*i as usize) < start);
        let last = selected.partition_point(|i| (*i as usize) < end);
        if first == last {
            out.push(nodes[node]);
        } else if size > 1 {
            emit(node * 2, start, size / 2, selected, nodes, out);
            emit(node * 2 + 1, start + size / 2, size / 2, selected, nodes, out);
        }
    }

    let mut proof = Vec::new();
    emit(1, 0, width, selected, &nodes, &mut proof);
    let leaves = selected
        .iter()
        .map(|i| (*i, payloads[*i as usize].to_vec()))
        .collect();
    Ok(SparseLeaves { leaves, proof })
}

pub fn prove(blob: &[u8], selected: &[u32]) -> Result<SparsePrelude, &'static str> {
    let opened = prove_in(RECORD, blob, selected)?;
    Ok(SparsePrelude {
        records: opened.leaves,
        proof: opened.proof,
        names: SparseLeaves::default(),
    })
}

pub fn verify(root: Hash, leaf_count: u32, sparse: &SparsePrelude) -> bool {
    verify_in(RECORD, root, leaf_count, &sparse.records, &sparse.proof)
}

fn verify_in(
    dom: Domain,
    root: Hash,
    leaf_count: u32,
    records: &[(u32, Vec<u8>)],
    proof: &[Hash],
) -> bool {
    if !records.windows(2).all(|w| w[0].0 < w[1].0)
        || records.iter().any(|(i, _)| *i >= leaf_count)
    {
        return false;
    }
    let width = (leaf_count as usize).max(1).next_power_of_two();
    let selected: Vec<u32> = records.iter().map(|(i, _)| *i).collect();
    let mut record_pos = 0usize;
    let mut proof_pos = 0usize;

    // 明示スタックによる後行順走査。
    //
    // 以前は再帰で書いていたが、訪れるノードが数千あり、引数 9 個の関数呼び出しの
    // オーバーヘッドがハッシュ本体を大きく上回っていた（実ハッシュ約 860K に対して
    // 走査が約 2.1M）。フレームを積む代わりに小さな enum をスタックに積む。
    //
    // `sel_lo..sel_hi` はこの部分木に落ちる `selected` の範囲。分割点だけを
    // 部分範囲から二分探索するので、探索は内部ノードあたり 1 回で済む。
    enum Step {
        Descend { start: usize, size: usize, lo: usize, hi: usize },
        Combine,
    }

    let iv = internal_iv();
    let mut work: Vec<Step> = Vec::with_capacity(64);
    // 走査中は `Digest` のまま持ち回り、変換は証明ノードの読み込み時だけにする。
    let mut out: Vec<risc0_zkvm::sha::Digest> = Vec::with_capacity(64);
    work.push(Step::Descend { start: 0, size: width, lo: 0, hi: selected.len() });

    while let Some(step) = work.pop() {
        match step {
            Step::Descend { start, size, lo, hi } => {
                if lo == hi {
                    // この部分木に選択レコードが無いので、証明中の部分木ハッシュを使う。
                    match proof.get(proof_pos) {
                        Some(h) => out.push(as_digest(h)),
                        None => return false,
                    }
                    proof_pos += 1;
                } else if size == 1 {
                    match records.get(record_pos) {
                        Some((idx, bytes)) if *idx as usize == start => {
                            out.push(leaf_digest(dom, *idx, bytes));
                            record_pos += 1;
                        }
                        _ => return false,
                    }
                } else {
                    let half = size / 2;
                    let mid = start + half;
                    let split =
                        lo + selected[lo..hi].partition_point(|i| (*i as usize) < mid);
                    // 左を先に処理させたいので、Combine → 右 → 左 の順に積む。
                    work.push(Step::Combine);
                    work.push(Step::Descend { start: mid, size: half, lo: split, hi });
                    work.push(Step::Descend { start, size: half, lo, hi: split });
                }
            }
            Step::Combine => {
                // 左が先に積まれているので、上にあるのが右。
                let right = match out.pop() {
                    Some(h) => h,
                    None => return false,
                };
                let left = match out.pop() {
                    Some(h) => h,
                    None => return false,
                };
                out.push(node_digest(&iv, &left, &right));
            }
        }
    }

    out.len() == 1
        && to_hash_d(&out[0]) == root
        && record_pos == records.len()
        && proof_pos == proof.len()
}

pub fn encode(sparse: &SparsePrelude) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(sparse.records.len() as u32).to_le_bytes());
    for (idx, bytes) in &sparse.records {
        out.extend_from_slice(&idx.to_le_bytes());
        out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        out.extend_from_slice(bytes);
    }
    out.extend_from_slice(&(sparse.proof.len() as u32).to_le_bytes());
    for hash in &sparse.proof {
        out.extend_from_slice(hash);
    }
    out.extend_from_slice(&(sparse.names.leaves.len() as u32).to_le_bytes());
    for (idx, bytes) in &sparse.names.leaves {
        out.extend_from_slice(&idx.to_le_bytes());
        out.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        out.extend_from_slice(bytes);
    }
    out.extend_from_slice(&(sparse.names.proof.len() as u32).to_le_bytes());
    for hash in &sparse.names.proof {
        out.extend_from_slice(hash);
    }
    out
}

pub fn decode(blob: &[u8]) -> Result<SparsePrelude, &'static str> {
    fn take<'a>(blob: &'a [u8], pos: &mut usize, n: usize) -> Result<&'a [u8], &'static str> {
        let out = blob.get(*pos..pos.checked_add(n).ok_or("sparse offset overflow")?)
            .ok_or("truncated sparse witness")?;
        *pos += n;
        Ok(out)
    }
    fn u32_at(blob: &[u8], pos: &mut usize) -> Result<u32, &'static str> {
        Ok(u32::from_le_bytes(take(blob, pos, 4)?.try_into().map_err(|_| "bad u32")?))
    }

    let mut pos = 0usize;
    let count = u32_at(blob, &mut pos)? as usize;
    let mut records = Vec::with_capacity(count);
    for _ in 0..count {
        let idx = u32_at(blob, &mut pos)?;
        let len = u32_at(blob, &mut pos)? as usize;
        records.push((idx, take(blob, &mut pos, len)?.to_vec()));
    }
    let proof_count = u32_at(blob, &mut pos)? as usize;
    let mut proof = Vec::with_capacity(proof_count);
    for _ in 0..proof_count {
        proof.push(take(blob, &mut pos, 32)?.try_into().map_err(|_| "bad hash")?);
    }
    let leaf_count = u32_at(blob, &mut pos)? as usize;
    let mut leaves = Vec::with_capacity(leaf_count);
    for _ in 0..leaf_count {
        let idx = u32_at(blob, &mut pos)?;
        let len = u32_at(blob, &mut pos)? as usize;
        leaves.push((idx, take(blob, &mut pos, len)?.to_vec()));
    }
    let name_proof_count = u32_at(blob, &mut pos)? as usize;
    let mut name_proof = Vec::with_capacity(name_proof_count);
    for _ in 0..name_proof_count {
        name_proof.push(take(blob, &mut pos, 32)?.try_into().map_err(|_| "bad hash")?);
    }
    if pos != blob.len() {
        return Err("trailing sparse witness bytes");
    }
    Ok(SparsePrelude {
        records,
        proof,
        names: SparseLeaves { leaves, proof: name_proof },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blob(parts: &[&[u8]]) -> Vec<u8> {
        let mut out = Vec::new();
        for part in parts {
            out.extend_from_slice(&(part.len() as u32).to_le_bytes());
            out.extend_from_slice(part);
        }
        out
    }

    #[test]
    fn multiproof_round_trip_and_tamper_rejection() {
        let input = blob(&[b"a", b"bb", b"ccc", b"dddd", b"eeeee"]);
        let (root, count) = root(&input).unwrap();
        let sparse = prove(&input, &[1, 4]).unwrap();
        assert!(verify(root, count, &sparse));
        assert_eq!(decode(&encode(&sparse)).unwrap(), sparse);

        let mut tampered = sparse.clone();
        tampered.records[0].1[0] ^= 1;
        assert!(!verify(root, count, &tampered));

        let mut missing = sparse.clone();
        missing.records.remove(0);
        assert!(!verify(root, count, &missing));
    }

    #[test]
    fn declaration_name_commitment_is_canonical() {
        let names = vec!["A".to_string(), "B.x".to_string()];
        assert!(names_root(&names).is_some());
        assert!(names_root(&["B".to_string(), "A".to_string()]).is_none());
        assert!(names_root(&["A".to_string(), "A".to_string()]).is_none());
    }

    fn name_set() -> Vec<String> {
        ["Nat.add", "Nat.mul", "Nat.sub", "Real.pi", "Zero.zero"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    #[test]
    fn non_membership_accepts_absent_names() {
        let names = name_set();
        let (root, count) = names_root(&names).unwrap();
        let queries = vec!["AAA.first".to_string(), "Nat.mod".to_string(), "zzz.last".to_string()];
        let opened = prove_names(&names, &queries).unwrap();
        assert!(verify_names(root, count, &opened));
        assert!(names_absent(&opened, count, &queries));
        // 開く葉は query 1 つにつき高々 2 枚。集合全体を渡していない。
        assert!(opened.leaves.len() <= 2 * queries.len());
        assert!(opened.leaves.len() < names.len());
    }

    #[test]
    fn non_membership_rejects_a_present_name() {
        let names = name_set();
        let (root, count) = names_root(&names).unwrap();
        let queries = vec!["Nat.mul".to_string()];
        let opened = prove_names(&names, &queries).unwrap();
        assert!(verify_names(root, count, &opened));
        assert!(!names_absent(&opened, count, &queries));
    }

    /// 隣接を見ないと、間に query 本人がいる葉の組で「無い」と言えてしまう。
    #[test]
    fn non_membership_requires_adjacent_leaves() {
        let names = name_set();
        let (root, count) = names_root(&names).unwrap();
        // Nat.add (0) と Nat.sub (2) を開く。間の Nat.mul (1) を飛ばしている。
        let blob = names_blob(&names).unwrap();
        let opened = prove_in(NAME, &blob, &[0, 2]).unwrap();
        assert!(verify_names(root, count, &opened));
        assert!(!names_absent(&opened, count, &["Nat.mul".to_string()]));
    }

    /// 端の名前は、その葉が本当に端であることまで確かめる。
    #[test]
    fn non_membership_checks_the_boundaries() {
        let names = name_set();
        let (_, count) = names_root(&names).unwrap();
        let blob = names_blob(&names).unwrap();
        // 最小の名前を問うのに、先頭でない葉を出しても通らない。
        let not_first = prove_in(NAME, &blob, &[1]).unwrap();
        assert!(!names_absent(&not_first, count, &["AAA.first".to_string()]));
        // 最大の名前を問うのに、末尾でない葉を出しても通らない。
        let not_last = prove_in(NAME, &blob, &[3]).unwrap();
        assert!(!names_absent(&not_last, count, &["zzz.last".to_string()]));
    }
}
