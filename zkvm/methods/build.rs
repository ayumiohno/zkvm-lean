//! Pin Stage 1's (envcheck) image ID into Stage 2 (thmcheck) at build time.
//!
//! If envcheck_id came from a private input, a prover could supply a malicious
//! Stage 1's image ID, have it declare any number of checked declarations, and make
//! Stage 2 skip real checks. The guest itself accepts only the correct ID.
//!
//! Making methods-env a build dependency ensures envcheck is built first and its
//! image ID is fixed.
//!
//! The file is generated inside the guest's source tree for the Docker build: the
//! container can see neither the host's environment variables nor `OUT_DIR`, so it
//! has to live under `root_dir`.
//!
//! NOTE: the generated file's own header text below is compiled into the guest, so it
//! is left in Japanese deliberately — changing it would change the image ID.

use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));

    let id = methods_env::ENVCHECK_ID;
    let generated = manifest.join("thmcheck/src/pinned_envcheck_id.rs");
    let contents = format!(
        "// このファイルは methods/build.rs が生成する。手で編集しない。\n\
         /// ビルド時に確定した Stage 1 の image ID。witness からは受け取らない。\n\
         pub const PINNED_ENVCHECK_ID: [u32; 8] = {id:?};\n"
    );
    // Skip the write when unchanged: every write makes cargo rebuild again.
    if std::fs::read_to_string(&generated).ok().as_deref() != Some(contents.as_str()) {
        std::fs::write(&generated, &contents).expect("failed to write pinned id");
    }

    println!("cargo:rerun-if-changed=build.rs");
    risc0_build::embed_methods_with_options(lean_zkvm_guest_options(&manifest));
}

/// Shared settings for reproducible builds.
///
/// With `LEAN_ZKVM_DOCKER=1`, the guest is built in Docker.
///
/// Local builds embed absolute paths in the ELF (file names for panic messages), so
/// **the image ID changes with the checkout location** and a verifier cannot confirm
/// it independently. Anything published is therefore built in Docker.
/// `--remap-path-prefix` needs the real path on its left-hand side and fits in neither
/// the guest's Cargo.toml nor `GuestOptions`, so fixing the path inside the container
/// is the only option.
fn lean_zkvm_guest_options(
    manifest: &std::path::Path,
) -> std::collections::HashMap<&'static str, risc0_build::GuestOptions> {
    let mut opts = risc0_build::GuestOptions::default();
    if std::env::var("LEAN_ZKVM_DOCKER").is_ok() {
        // The repository root, because the path dependencies (forks/nanoda_lib and
        // zkvm/stmt) all have to be inside the build context.
        let root = manifest.join("../..").canonicalize().expect("repo root");
        opts.use_docker = Some(
            risc0_build::DockerOptionsBuilder::default()
                .root_dir(root)
                .build()
                .expect("docker options"),
        );
    }
    ["method", "thmcheck", "parseprobe"].into_iter().map(|k| (k, opts.clone())).collect()
}
