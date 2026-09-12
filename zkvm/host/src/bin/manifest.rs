//! Write the official image IDs out as a manifest.
//!
//!   manifest [output.json]
//!
//! A verifier should not have to build the guest, so the image IDs are distributed as
//! data. The project publishes this per release and verifiers pass it to
//! `verify --manifest`.
//!
//! The manifest is itself a trusted input — it declares which guest is authoritative —
//! so its distribution must be backed by signatures or repository tags.

use risc0_zkvm::sha::Digest;

fn main() {
    let path = std::env::args().nth(1).unwrap_or_else(|| "manifest.json".into());
    let json = serde_json::json!({
        "envcheck_id": Digest::from(methods::ENVCHECK_ID).to_string(),
        "thmcheck_id": Digest::from(methods::THMCHECK_ID).to_string(),
        "permitted_axioms": stmt::policy::PERMITTED_AXIOMS,
        "risc0": "3.0.6",
        "lean_toolchain": "leanprover/lean4:v4.34.0-rc2",
    });
    let text = serde_json::to_string_pretty(&json).unwrap();
    std::fs::write(&path, format!("{text}\n")).expect("cannot write manifest");
    println!("{text}");
    println!();
    println!("wrote {path}");
}
