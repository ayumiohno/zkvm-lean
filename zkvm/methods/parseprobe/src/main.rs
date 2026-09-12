//! A guest for breaking down parse cost.
//!
//! nanoda's parse has two stages: parsing the JSON, and ingesting it into the DAG
//! (hash-consing). Which one dominates decides what to optimise, so JSON parsing is
//! measured on its own.

mod io;

use risc0_zkvm::guest::env;

fn main() {
    let ndjson = io::read_bytes();

    // With empty input, measure the per-call floor for SHA.
    if ndjson.len() <= 8 {
        use risc0_zkvm::sha::{Digest, Impl, Sha256};
        const N: usize = 2000;
        let a = Digest::from([1u32; 8]);
        let b = Digest::from([2u32; 8]);
        let iv = Digest::from([3u32; 8]);

        let t0 = env::cycle_count();
        let mut acc = Digest::ZERO;
        for _ in 0..N {
            acc = *Impl::compress(&iv, &a, &b);
        }
        let t1 = env::cycle_count();

        let small = [7u8; 53];
        for _ in 0..N {
            acc = *Impl::hash_bytes(&small);
        }
        let t2 = env::cycle_count();

        let bulk = [7u8; 53 * N];
        let _ = Impl::hash_bytes(&bulk);
        let t3 = env::cycle_count();

        // Cost of converting Hash([u8;32]) <-> Digest. node_hash goes through three
        // try_from calls and one as_bytes per invocation, so this may matter.
        let raw = [9u8; 32];
        let mut sink = 0u32;
        for _ in 0..N {
            let d = Digest::try_from(raw.as_slice()).unwrap();
            sink ^= d.as_words()[0];
        }
        let t4 = env::cycle_count();
        let d = Digest::from([5u32; 8]);
        for _ in 0..N {
            let b: [u8; 32] = d.as_bytes().try_into().unwrap();
            sink ^= b[0] as u32;
        }
        let t5 = env::cycle_count();
        env::log(&format!(
            "convprobe: try_from={} ({}/call)  as_bytes={} ({}/call)  sink={}",
            t4 - t3, (t4 - t3) / N as u64,
            t5 - t4, (t5 - t4) / N as u64,
            sink,
        ));

        env::log(&format!(
            "shaprobe: compress={} ({}/call)  hash_bytes53={} ({}/call)  bulk{}B={} ({}/call equiv)",
            t1 - t0, (t1 - t0) / N as u64,
            t2 - t1, (t2 - t1) / N as u64,
            53 * N, t3 - t2, (t3 - t2) / N as u64,
        ));
        env::commit(&(acc.as_words()[0] as u64, 0u64));
        return;
    }


    // For the binary format, measure record decoding on its own
    if ndjson.first() != Some(&b'{') {
        let t0 = env::cycle_count();
        let mut pos = 0usize;
        let mut n = 0u64;
        while pos < ndjson.len() {
            let len = u32::from_le_bytes(ndjson[pos..pos + 4].try_into().unwrap()) as usize;
            pos += 4;
            let _rec: nanoda_lib::parser::ExportRecord =
                postcard::from_bytes(&ndjson[pos..pos + len]).unwrap();
            pos += len;
            n += 1;
        }
        let t1 = env::cycle_count();
        env::log(&format!(
            "binprobe: bytes={} records={} decode_only={}",
            ndjson.len(),
            n,
            t1 - t0
        ));
        env::commit(&(n, 0u64));
        return;
    }

    let text = std::str::from_utf8(&ndjson).unwrap();

    // (1) splitting into lines only
    let t0 = env::cycle_count();
    let mut n_lines = 0u64;
    let mut total = 0u64;
    for line in text.lines() {
        n_lines += 1;
        total += line.len() as u64;
    }
    let t1 = env::cycle_count();

    // (2) via the generic serde_json::Value (reference only; not nanoda's path)
    let mut n_parsed = 0u64;
    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        let v: serde_json::Value = serde_json::from_str(line).unwrap();
        n_parsed += v.as_object().map_or(0, |o| o.len()) as u64;
    }
    let t2 = env::cycle_count();

    // (3) nanoda's own typed parse alone, skipping ingestion
    let n_typed =
        nanoda_lib::parser::parse_json_only(std::io::BufReader::new(std::io::Cursor::new(&ndjson)))
            .unwrap();
    let t3 = env::cycle_count();

    env::log(&format!(
        "parseprobe: bytes={} lines={} | split={} value_json={} typed_json={} (records={}, fields={})",
        ndjson.len(),
        n_lines,
        t1 - t0,
        t2 - t1,
        t3 - t2,
        n_typed,
        n_parsed,
    ));
    env::commit(&(n_lines, total));
}
