//! Build Stage 1 (envcheck).
//!
//! It is a separate crate from methods so its image ID can be baked into thmcheck.
//! The reproducible-build settings must stay in step with methods/build.rs.

use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    let mut opts = risc0_build::GuestOptions::default();
    if std::env::var("LEAN_ZKVM_DOCKER").is_ok() {
        let root = manifest.join("../..").canonicalize().expect("repo root");
        opts.use_docker = Some(
            risc0_build::DockerOptionsBuilder::default()
                .root_dir(root)
                .build()
                .expect("docker options"),
        );
    }
    println!("cargo:rerun-if-changed=build.rs");
    risc0_build::embed_methods_with_options([("envcheck", opts)].into_iter().collect());
}
