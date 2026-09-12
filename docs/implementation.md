# Implementation

## Setup

```sh
curl -sSf https://sh.rustup.rs | sh -s -- -y                   # Rust
curl -L https://risczero.com/install | bash && rzup install    # RISC Zero
scripts/setup.sh                                               # clone the exporter, build everything
```

`scripts/setup.sh` clones lean4export and builds it together with the tracked
`forks/nanoda_lib`. The Lean toolchain is **v4.34.0-rc2**, matching lean4export.

### Proving on GPU (63×)

Enable the `cuda` feature on `host`. A CUDA toolkit (nvcc) is required.

```sh
export PATH="/usr/local/cuda/bin:$PATH"
cargo build --release --features cuda      # first build compiles CUDA kernels, ~30 min
```

`cuda` pulls in `risc0-zkvm`'s `prove`, so `default_prover()` selects the in-process
`LocalProver` (CUDA) instead of `r0vm` over IPC. No code change and no `RISC0_PROVER`
setting is needed.

Measured at about **370K cycles/s** on a Quadro RTX 8000 — 63× the M4 Mac CPU.

Where clang's builtin headers are missing, bindgen in the `cuda` → `prove` →
`risc0-groth16/prove` → `circom-witnesscalc` chain fails with `stddef.h not found`.
Point it at gcc's:

```sh
export BINDGEN_EXTRA_CLANG_ARGS="-I/usr/lib/gcc/x86_64-linux-gnu/13/include"
```

## Layout

```
lean/              sample theorems for benchmarking (ZkDemo)
examples/
  mathlib/         Mathlib-based examples
    MathlibDemo.lean   ring / linarith (tactic cost)
    Practical.lean     infinitude of primes, irrationality of √2, factorisation, …
  preimage/        the "I know a preimage" example; run.sh / verify.sh
  zkvm-hello/      minimal zkVM example, no Lean, runs in 5 seconds
vendor/            lean4export (untracked; setup.sh provides it)
forks/nanoda_lib/  research fork of nanoda_lib (tracked)
zkvm/
  stmt/            canonical statement digests and the pinned type-checking policy
                   (shared by the guest and the verifier tools)
  methods-env/
    envcheck/      Stage 1: check the prelude and commit to its contents
                   (a separate crate so its image ID can be baked into thmcheck)
  methods/
    guest/         single-stage baseline, for comparison
    thmcheck/      Stage 2: inherit Stage 1 and check only the theorem
    parseprobe/    parse-cost instrumentation
  host/            drivers and tools (below)
scripts/           setup / export / bench
contracts/         on-chain verification (TheoremBounty.sol, gas.sh)
docs/              this documentation
```

## Tools

All binaries land in `zkvm/target/release/`.

| | |
| --- | --- |
| `host <export.bin> [--prove] [--composite]` | single stage; executor only by default |
| `compose <prelude.bin> <full.bin> <theorem> [opts]` | two stages; options below |
| `manifest [out.json]` | write the official image IDs as JSON, for verifiers |
| `tobin <in.ndjson> <out.bin>` | NDJSON → binary format |
| `verify <receipt.bin> <public.bin> <expected.ndjson> <theorem> [--manifest <path>] [--require-certified]` | **for verifiers.** All six checks. `--require-certified` rejects `assumed` |
| `checkprelude <public.bin>` | **for verifiers.** Type-check the public prelude natively; no Lean needed |
| `inspect <receipt.bin>` | show a receipt's structure and journal contents |
| `calldata <receipt.bin>` | extract `imageId` / `journalDigest` / `seal` for on-chain use |
| `stmtdigest <export.ndjson> <theorem>` | compute a canonical statement digest |
| `checkbin <export.ndjson>` | confirm the NDJSON and binary paths agree |
| `checkprofile` · `touched` · `prune` · `stage2` · `parseprobe` | measurement tools |

### `compose` options

| | |
| --- | --- |
| `--prove` | actually generate a proof (default is executor only) |
| `--env-receipt <path>` | **read a Stage 1 receipt; create and save one if absent** |
| `--out <path>` | write the Stage 2 receipt — the only thing a verifier receives |
| `--groth16` | compress Stage 2 to Groth16, for on-chain verification |
| `--full` | pass the full prelude instead of a sparse witness (A/B comparison) |
| `--assume-prelude` | **skip Stage 1 (recommended).** The verifier runs `checkprelude` themselves. The only option at Mathlib scale |

**`--env-receipt` is where composition pays off.** A receipt is reused when its
checked set covers the declarations this theorem needs; otherwise a new Stage 1 is
produced. On reuse the journal's prelude digest is compared against the local prelude,
so a receipt from a different environment cannot be recycled.

```
$ compose out/A_f.bin out/B_both.bin Preimage.knows_preimage --env-receipt out/env_receipt.bin
assertion failed: the stored env receipt belongs to a different prelude
```

## Reproducible builds (Docker)

Local builds embed absolute paths in the ELF, so **the image ID changes with the
checkout location**. Anything published must be built with Docker.

```sh
LEAN_ZKVM_DOCKER=1 cargo build --release
```

**If the same machine also proves, pass `--features cuda` as well.**
`scripts/build-release.sh` does not pass cuda to the host, so running it as-is falls
back to CPU proving — three minutes becomes three hours at Mathlib scale.

```sh
LEAN_ZKVM_DOCKER=1 cargo build --release --features cuda   # Docker guest, CUDA host
```

`root_dir` is the repository root, because the path dependencies (`forks/nanoda_lib`
and `zkvm/stmt`) all have to be inside the build context.

The file that bakes in Stage 1's image ID is generated at
`methods/thmcheck/src/pinned_envcheck_id.rs` (untracked) rather than in `OUT_DIR`,
because the container can see neither the host's environment variables nor `OUT_DIR`.

See [soundness](soundness.md) for the reproducibility measurements.

## Verifiers do not build the guest

If `verify` linked against `methods`, verifying would require installing a RISC-V
toolchain and building the prover's code. `manifest` emits the image IDs as data
instead, and `verify --manifest` consumes them.

```json
{
  "envcheck_id": "e701c261a98b3354...",
  "thmcheck_id": "23370ed42978e0cb...",
  "permitted_axioms": ["propext", "Classical.choice", "Quot.sound", "Lean.trustCompiler"],
  "risc0": "3.0.6",
  "lean_toolchain": "leanprover/lean4:v4.34.0-rc2"
}
```

The manifest declares which guest is authoritative, so it is a **trusted input**;
distribute it via repository tags or signatures. Tampering makes verification fail:

```
$ verify ... --manifest /tmp/bad_manifest.json
❌ verification failed: receipt is invalid for the thmcheck image id
```

## End-to-end

```sh
# 1. write and build the theorem
cd examples/preimage && lake build

# 2. export (constants are emitted in the order given after --, so
#    public.ndjson is a byte prefix of full.ndjson)
lake env <exporter> Preimage -- Preimage.f                          > public.ndjson
lake env <exporter> Preimage -- Preimage.f Preimage.knows_preimage  > full.ndjson

# 3. convert to the binary format
tobin public.ndjson public.bin && tobin full.ndjson full.bin

# 4. run the zkVM (executor by default; --prove for a real proof)
compose public.bin full.bin Preimage.knows_preimage --prove \
        --env-receipt env_receipt.bin --out receipt.bin

# 5. verifier side
#    (a) type-check the public prelude once; no Lean installation needed
checkprelude public.bin
#    (b) verify the receipt — the only artefact received from the prover
verify receipt.bin public.bin expected.ndjson Preimage.knows_preimage --manifest manifest.json
```

`examples/preimage/run.sh` and `verify.sh` run exactly this.

## Inside the guest

Stage 2 (`methods/thmcheck/src/main.rs`) is the core.

```rust
// Policy and Stage 1's image ID are pinned in the guest, never read from the witness.
include!(env!("PINNED_ENVCHECK_ID_FILE"));

let sparse_mode = env::read();
let prelude_input = io::read_bytes();    // full prelude, or a sparse multiproof
let suffix  = io::read_bytes();

env::verify(PINNED_ENVCHECK_ID, &env_journal);     // inherit Stage 1
if sparse_mode {
    verify(record_root, record_count, sparse);       // membership
    verify(declaration_names_hash, public_names);    // anti-shadowing
    all = compact_sparse_export(sparse.records, suffix);
} else {
    assert_eq!(sha256(prelude_input), env_digest);
    all = prelude_input ++ suffix;
}

let export_file = parse_export_file_binary(all, stmt::policy::config());

touch_trace::reset();
export_file.check_declars_skipping(compact_prelude_declars);
assert!(name_certificate_covers(checked_prelude, touched_names));
assert!(suffix_avoids_public_names(public_names, suffix_names));

let d = stmt::statement_digest(&export_file, &target_name);
env::commit(&(envcheck_id, prelude_digest, target_name, d, n, skipped));
```

Skipping either the full-mode SHA comparison or the sparse-mode Merkle membership
would allow a receipt from another environment to be reused.

**Must never come from the witness** (see [soundness](soundness.md)):

- the type-checking policy — `unsafe_permit_all_axioms` could be set;
- Stage 1's image ID — a malicious Stage 1 could declare any number of checked
  declarations.

## The nanoda_lib fork

`forks/nanoda_lib` branches from upstream commit `0505569`, with changes tracked
directly in git.

| | Purpose |
| --- | --- |
| `Config::to_export_file_from_reader` / `parse_export_file` made `pub` | a zkVM guest has no filesystem and no stdin, so input must come from bytes |
| `check_declars_skipping(n)` | skip re-checking the first n declarations under composition |
| `ExportRecord` / `ndjson_to_binary` / `parse_export_file_binary` | a record type without `#[serde(flatten)]`, encoded with postcard; ingestion (`apply_record`) is shared with the JSON path |
| `serialize_biguint_as_string` | the counterpart to `deserialize_biguint_from_string`; without it the round trip fails |
| `check_declar_closure(roots, limit)` | iterate a worklist to a fixed point while tracing lookups, checking only the selected prelude closure |
| `select_sparse_prelude_records` | select the static closure of records needed to parse, given the suffix and required declarations |
| `compact_sparse_export` | remap authenticated records' back-references to dense indices so parsing works without materialising unselected records |
| `prune_binary` / `insert_placeholder` | build pruned witnesses (measurement) |
| `parse_json_only`, `touch_trace` feature (off by default) | instrumentation |

Stage 1's journal is `(prelude_digest, record_root, record_count,
declaration_names_hash, total_declars, checked_names, skipped_axioms)`. The root set
the host proposes is not trusted: Stage 1 adds transitive dependencies itself, and
Stage 2 re-measures real lookups during theorem checking, halting if any reference
falls outside `checked_names`.

`stmt::merkle` puts the record index, payload length and payload into a
domain-separated SHA-256 leaf. Subtrees containing no selected leaf collapse to a
single hash in the compact multiproof, and records reach nanoda's compact parser only
after the root has been reconstructed.

Thread parallelism only starts when `num_threads > 1`, so `num_threads: 1` runs
unchanged in the zkVM.

## Pitfalls

- **The real work happens in `r0vm`, not `host`.** `host` is a thin socket client and
  barely uses CPU; watch `r0vm` for progress.
- **`env::read_frame` is unstable.** Use `env::read::<u32>()` + `env::read_slice()`.
- **`ProverOpts::succinct()` is required for composition.** Composite receipts cannot
  resolve assumptions.
- **Without a Stage 1 receipt, `--prove` yields a conditional receipt**, which cannot
  be verified on its own.
- **Whatever the guest reads becomes the trust boundary.** Conditions of verification —
  policy, image IDs — must never arrive by witness.
- Changes to nanoda go in `forks/nanoda_lib/`, not `vendor/`, which is an untracked
  clone target.
