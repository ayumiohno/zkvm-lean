//! The host-side driver.
//!
//! `cargo run --release -- <export.ndjson> [--prove]`
//!
//! By default only the executor runs, counting cycles without producing a proof.
//! A proof is generated only with `--prove`.

use methods::{METHOD_ELF, METHOD_ID};
use risc0_zkvm::{default_executor, default_prover, ExecutorEnv, ProverOpts};
use std::time::Instant;

/// The write side matching the guest's `io::read_bytes`, bypassing serde.
fn write_bytes(builder: &mut risc0_zkvm::ExecutorEnvBuilder, data: &[u8]) {
    builder.write(&(data.len() as u32)).unwrap();
    let mut padded = data.to_vec();
    padded.resize(data.len().div_ceil(4) * 4, 0);
    builder.write_slice(bytemuck::cast_slice::<u8, u32>(&padded));
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let mut args = std::env::args().skip(1);
    let path = args.next().expect("usage: host <export.ndjson> [--prove]");
    let rest: Vec<String> = args.collect();
    let do_prove = rest.iter().any(|a| a == "--prove");
    // Succinct costs more because of recursive compression. For a first run-through,
    // --composite yields an uncompressed receipt: still verifiable, just larger.
    let composite = rest.iter().any(|a| a == "--composite");

    let ndjson = std::fs::read(&path).expect("cannot read export file");

    let mut builder = ExecutorEnv::builder();
    write_bytes(&mut builder, &ndjson);
    let env = builder.build().unwrap();

    println!("input: {} ({} bytes)", path, ndjson.len());

    let t = Instant::now();
    if do_prove {
        let prover = default_prover();
        let prove_info = if composite {
            prover.prove(env, METHOD_ELF).expect("proving failed")
        } else {
            prover
                .prove_with_opts(env, METHOD_ELF, &ProverOpts::succinct())
                .expect("proving failed")
        };
        let elapsed = t.elapsed();
        prove_info.receipt.verify(METHOD_ID).expect("verification failed");
        println!(
            "PROVED  cycles={} elapsed={:.1}s journal={} bytes",
            prove_info.stats.total_cycles,
            elapsed.as_secs_f64(),
            prove_info.receipt.journal.bytes.len()
        );
    } else {
        let session = default_executor()
            .execute(env, METHOD_ELF)
            .expect("execution failed");
        println!(
            "EXECUTED cycles={} segments={} elapsed={:.1}s journal={} bytes",
            session.cycles(),
            session.segments.len(),
            t.elapsed().as_secs_f64(),
            session.journal.bytes.len()
        );
    }
}
