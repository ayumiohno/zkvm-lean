//! The independent verifier.
//!
//!   verify <receipt.bin> <public.bin> <expected export.ndjson> <theorem> [--manifest <path>]
//!
//! The receipt is the only thing received from the prover, and it contains no proof
//! term. The verifier supplies, themselves:
//!
//!   - `public.bin`     the public environment (prelude) they trust
//!   - `export.ndjson`  the proposition they wrote (proof may be `sorry`)
//!   - `theorem`        the name the journal must carry
//!
//! `--expected-name <n>` names the declaration to compare against inside the expected
//! export, when it differs from the journal's. `--declarations <n>` is how many
//! declarations the private suffix may contain (default 1).
//!
//! `--manifest` (the JSON emitted by the `manifest` command) lets a verifier compare
//! image IDs without building the guest. Omitted, the build-time values are used.
//!
//! **All** of the following are checked. Dropping any one opens a hole.
//!
//!   1. the receipt is valid for THMCHECK_ID (the intended guest ran)
//!   2. the journal's env image ID is the official ENVCHECK_ID
//!   3. the journal's prelude digest, record root and name root all match the
//!      verifier's public.bin
//!   4. the theorem name is the expected one
//!   5. the canonical statement digest matches the verifier's own named proposition
//!   6. no axioms outside the allowlist were used
//!   7. the private suffix cannot have introduced an allowlisted axiom
//!   8. every constant the statement refers to is resolved by the public environment
//!   9. the verifier's file and the public environment agree on what those constants mean

use methods::{ENVCHECK_ID, THMCHECK_ID};
use nanoda_lib::parser::parse_export_file_binary;
use risc0_zkvm::sha::{Digest, Impl, Sha256};
use risc0_zkvm::Receipt;
use std::io::{BufReader, Cursor};

/// Parse a hex string into a Digest. `Digest` does not implement `FromStr`.
fn digest_from_hex(s: &str) -> Digest {
    if s.len() != 64 {
        fail(&format!("image id must be 64 hex digits: {s}"));
    }
    let bytes: Vec<u8> = (0..32)
        .map(|i| {
            u8::from_str_radix(&s[i * 2..i * 2 + 2], 16)
                .unwrap_or_else(|e| fail(&format!("image id is not hex: {e}")))
        })
        .collect();
    Digest::try_from(bytes.as_slice()).unwrap_or_else(|e| fail(&format!("malformed image id: {e}")))
}

fn fail(msg: &str) -> ! {
    eprintln!("❌ verification failed: {msg}");
    std::process::exit(1);
}

#[path = "../audit.rs"]
mod audit;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    if a.len() < 4 {
        eprintln!("usage: verify <receipt.bin> <public.bin> <expected export.ndjson> <theorem>");
        std::process::exit(2);
    }
    let positional: Vec<&String> = a.iter().filter(|s| !s.starts_with("--")).collect();
    if positional.len() < 4 {
        eprintln!("usage: verify <receipt.bin> <public.bin> <expected export.ndjson> <theorem>\n       [--manifest <path>] [--require-certified] [--expected-name <name>] [--declarations <n>]");
        std::process::exit(2);
    }
    let (receipt_path, public_path, expected_path, target) =
        (positional[0], positional[1], positional[2], positional[3]);
    let flag = |name: &str| -> Option<String> {
        a.iter().position(|s| s == name).and_then(|i| a.get(i + 1)).cloned()
    };
    // The declaration to compare against inside the expected export. The verifier may
    // name their own copy of the proposition differently; they still have to say which
    // one it is, because the expected export holds the whole transitive closure.
    let expected_name = flag("--expected-name").unwrap_or_else(|| target.clone());
    // How many declarations the private suffix may contain. One means the target
    // theorem and nothing else, which leaves no room for an injected axiom.
    let max_suffix: u64 = flag("--declarations")
        .map(|v| v.parse().unwrap_or_else(|e| fail(&format!("--declarations: {e}"))))
        .unwrap_or(1);

    // A manifest, when given, is authoritative — no need to build the guest.
    let manifest = a.iter().position(|s| s == "--manifest").and_then(|i| a.get(i + 1)).map(|p| {
        let text = std::fs::read_to_string(p).unwrap_or_else(|e| fail(&format!("manifest: {e}")));
        serde_json::from_str::<serde_json::Value>(&text)
            .unwrap_or_else(|e| fail(&format!("manifest is not JSON: {e}")))
    });
    let expect_id = |key: &str, built_in: [u32; 8]| -> Digest {
        match manifest.as_ref() {
            Some(m) => {
                let s = m[key].as_str().unwrap_or_else(|| fail(&format!("manifest has no {key}")));
                digest_from_hex(s)
            }
            None => Digest::from(built_in),
        }
    };
    let want_thmcheck = expect_id("thmcheck_id", THMCHECK_ID);
    let want_envcheck = expect_id("envcheck_id", ENVCHECK_ID);
    if manifest.is_some() {
        println!("using image IDs from the manifest");
    }

    let receipt: Receipt = bincode::deserialize(&std::fs::read(receipt_path).unwrap_or_else(|e| {
        fail(&format!("cannot read the receipt: {e}"))
    }))
    .unwrap_or_else(|e| fail(&format!("cannot decode the receipt: {e}")));

    // 1. Verify the receipt itself. A conditional receipt fails here.
    receipt
        .verify(want_thmcheck)
        .unwrap_or_else(|e| fail(&format!("receipt is invalid for the thmcheck image id: {e}")));
    println!("✅ 1. receipt is valid (image id {want_thmcheck})");

    let (certified, env_id, prelude_digest, record_root, names_root, name, statement, n_declars, skipped): (
        bool,
        [u32; 8],
        Digest,
        [u8; 32],
        [u8; 32],
        String,
        [u8; 32],
        u64,
        Vec<String>,
    ) = receipt.journal.decode().unwrap_or_else(|e| fail(&format!("cannot read the journal: {e}")));

    // 2. The environment trust mode.
    //
    //    Under `certified`, Stage 1 type-checked the prelude. Under `assumed`, that
    //    the prelude type-checks is presupposed. Which one to accept is the
    //    verifier's policy, enforced with `--require-certified`.
    if certified {
        if Digest::from(env_id) != want_envcheck {
            fail(&format!(
                "envcheck image id differs\n   expected {want_envcheck}\n   actual   {}",
                Digest::from(env_id)
            ));
        }
        println!("✅ 2. environment is certified (Stage 1 type-checked the prelude; image id matches)");
        println!(
            "⚠️     Stage 2 reconciles its lookups against Stage 1's certificate on flattened\n       \
             names (`stmt::name_certificate_covers`), and distinct kernel names can flatten\n       \
             onto one string. That comparison accepts on a hit, so it belongs in the guest\n       \
             and is not something this verifier can close."
        );
    } else {
        if a.iter().any(|s| s == "--require-certified") {
            fail("environment is assumed, but --require-certified was given");
        }
        println!(
            "⚠️  2. environment is assumed (no Stage 1)\n                   That this prelude type-checks is presupposed, not proved.\n                   Reasonable for a standard prelude; for a hand-built one, use --require-certified"
        );
    }

    // 3. Is the environment identical to the verifier's public prelude?
    //    Skipping this leaves a substituted definition of f undetectable.
    let public = std::fs::read(public_path).unwrap_or_else(|e| fail(&format!("public.bin: {e}")));
    let want_prelude = *Impl::hash_bytes(&public);
    if prelude_digest != want_prelude {
        fail(&format!(
            "prelude differs (a definition may have been substituted)\n   expected {want_prelude}\n   actual   {prelude_digest}"
        ));
    }
    // Compare the Merkle root too: under sparse input it is the real commitment.
    let (want_root, _count) =
        stmt::merkle::root(&public).unwrap_or_else(|e| fail(&format!("cannot compute the root: {e}")));
    if record_root != want_root {
        fail("the prelude Merkle root does not match the verifier's public environment");
    }
    // Compare the declaration-name root as well. **Skipping this opens a hole under
    // sparse input**: the guest sees only selected records, so a prover who lied
    // about the name set could redefine an unread public declaration in the private
    // suffix.
    let (public_ef, mut public_skipped) = parse_export_file_binary(&public, stmt::policy::config())
        .unwrap_or_else(|e| fail(&format!("cannot parse public.bin: {e}")));
    let mut public_names: Vec<String> = public_ef.with_ctx(|ctx| {
        public_ef
            .declars
            .values()
            .map(|d| stmt::name_to_string(ctx, d.info().name))
            .collect()
    });
    public_skipped.sort();
    public_names.extend(public_skipped);
    public_names.sort();
    let (want_names_root, _names_count) = stmt::merkle::names_root(&public_names)
        .unwrap_or_else(|| fail("declaration names in the public environment are not unique"));
    if names_root != want_names_root {
        fail("the declaration-name Merkle root does not match the verifier's public environment");
    }
    println!(
        "✅ 3. prelude matches the verifier's public environment ({} bytes; SHA-256, record root, name root)",
        public.len()
    );

    // 4. The theorem name.
    if name != *target {
        fail(&format!("theorem name differs: expected {target}, actual {name}"));
    }
    println!("✅ 4. theorem name matches: {name}");

    // 5. The canonical statement digest, computed from the verifier's own proposition.
    let expected_bytes =
        std::fs::read(expected_path).unwrap_or_else(|e| fail(&format!("expected export: {e}")));
    let expected_bin = if expected_path.ends_with(".ndjson") {
        nanoda_lib::parser::ndjson_to_binary(BufReader::new(Cursor::new(&expected_bytes)))
            .unwrap_or_else(|e| fail(&format!("cannot convert the expected export: {e}")))
    } else {
        expected_bytes
    };
    let (ef, _) = parse_export_file_binary(&expected_bin, stmt::policy::config())
        .unwrap_or_else(|e| fail(&format!("cannot parse the expected export: {e}")));
    // Compare against **one** named declaration.
    //
    // Searching the file for any declaration whose type matches would accept a proof of
    // something else entirely: an expected export carries the whole transitive closure
    // of the proposition, so trivially provable types (a constructor's, say) sit right
    // next to the one that was meant. The name has to be given, and it has to resolve
    // to exactly one declaration — `name_to_string` can flatten distinct kernel names
    // onto the same string, so an ambiguous hit is refused rather than resolved.
    match audit::count_declars_named(&ef, &expected_name) {
        1 => {}
        0 => fail(&format!("{expected_path} has no declaration named {expected_name}")),
        n => fail(&format!(
            "{expected_name} resolves to {n} declarations in {expected_path}; the name does not identify a proposition"
        )),
    }
    let want_stmt = stmt::statement_digest(&ef, &expected_name)
        .unwrap_or_else(|| fail(&format!("cannot read the type of {expected_name}")));
    if want_stmt != statement {
        fail(&format!(
            "statement mismatch\n   {expected_name} in {expected_path} has digest {}\n   the journal says {}",
            stmt::hex(&want_stmt),
            stmt::hex(&statement)
        ));
    }
    println!("✅ 5. statement digest matches {expected_name}: {}", stmt::hex(&statement));

    // 6. axiom。
    let extra: Vec<&String> =
        skipped.iter().filter(|a| !stmt::policy::PERMITTED_AXIOMS.contains(&a.as_str())).collect();
    if !extra.is_empty() {
        fail(&format!("the export contains axioms outside the allowlist: {extra:?}"));
    }
    println!("✅ 6. no axioms outside the allowlist");

    // 7. The private suffix must not be able to introduce an allowlisted axiom.
    //
    //    The parser admits an axiom on its name alone, and the guest's disjointness
    //    check only stops a suffix from redefining a name that the public environment
    //    *has*. So an allowlisted name that the prelude does not declare can be
    //    declared by the suffix — `axiom Lean.trustCompiler : ∀ p : Prop, p` — and it
    //    is permitted, is not reported as skipped, and proves anything.
    //
    //    Two ways to close it, either is enough:
    //      (a) the suffix is small enough to hold nothing but the theorem, or
    //      (b) the public environment already declares every allowlisted axiom, with
    //          the pinned type, so the disjointness check rejects any of them.
    let audited_all = stmt::policy::PERMITTED_AXIOMS.iter().all(|name| {
        match (audit::axiom_fingerprint(&public_ef, name), audit::pinned(name)) {
            (Some(actual), Some(want)) => actual == want,
            _ => false,
        }
    });
    // One declaration is the theorem itself and nothing else: check 4 pins its name,
    // and an axiom under that name would be outside the allowlist, so it would be
    // skipped rather than admitted and the guest would not find the target at all.
    // Any larger suffix has room for a declaration the verifier never sees.
    let closed = audited_all || (!certified && n_declars <= 1);
    if !closed {
        if certified {
            fail(
                "the suffix size cannot be bounded from the journal under `certified` (the count\n   \
                 includes Stage 1's declarations), and the public environment does not declare every\n   \
                 allowlisted axiom, so the suffix could have introduced one.",
            );
        }
        if n_declars > max_suffix {
            fail(&format!(
                "the private suffix holds {n_declars} declarations (at most {max_suffix} accepted), and the public\n   \
                 environment does not declare every allowlisted axiom with its pinned type.\n   \
                 The suffix could therefore have declared one itself and proved anything."
            ));
        }
    }
    if audited_all {
        println!("✅ 7. the public environment declares every allowlisted axiom, so the suffix cannot introduce one");
    } else if closed {
        println!("✅ 7. the private suffix holds exactly the theorem; no room for an injected axiom");
    } else {
        println!(
            "⚠️  7. the private suffix holds {n_declars} declarations (allowed by --declarations {max_suffix})\n                   \
             The public environment does not declare every allowlisted axiom, so one of those\n                   \
             declarations could be an allowlisted axiom with a forged type. Accepting this\n                   \
             receipt means trusting the prover on that point."
        );
    }

    // 8. Every constant the statement mentions must be resolved by the public
    //    environment.
    //
    //    The statement digest pins constants by name, not by definition. If a name the
    //    statement refers to is absent from the public prelude, the prover supplies its
    //    definition in the private suffix — `MyClaim := True` — and the digests still
    //    agree. Requiring the name to be public forces the prover to use the verifier's
    //    definition, because the prelude bytes are already pinned by check 3 and the
    //    guest refuses a suffix that redefines a public name.
    //
    //    Compared structurally: `name_to_string` flattens `Str(Anon, "Foo.bar")` and
    //    `Str(Str(Anon, "Foo"), "bar")` onto one string, and this test accepts on a hit.
    let public_keys = audit::name_keys(&public_ef);
    let stmt_constants = audit::statement_constants(&ef, &expected_name)
        .unwrap_or_else(|| fail(&format!("cannot read the type of {expected_name}")));
    let private: Vec<String> = ef.with_ctx(|ctx| {
        let named: Vec<(audit::NameKey, String)> = ef
            .declars
            .values()
            .map(|d| (audit::name_key(ctx, d.info().name), stmt::name_to_string(ctx, d.info().name)))
            .collect();
        stmt_constants
            .iter()
            .filter(|k| public_keys.binary_search(k).is_err())
            .map(|k| {
                named
                    .iter()
                    .find(|(nk, _)| nk == k)
                    .map(|(_, n)| n.clone())
                    .unwrap_or_else(|| "<unnamed>".to_string())
            })
            .collect()
    });
    if !private.is_empty() {
        fail(&format!(
            "the statement refers to constants that the public environment does not define: {private:?}\n   \
             Their definitions would come from the prover's private suffix, so the digest pins only the name.\n   \
             Put them in public.bin, or state the proposition in terms of what is already public."
        ));
    }
    println!(
        "✅ 8. all {} constants in the statement are defined by the public environment",
        stmt_constants.len()
    );

    // 9. The statement's constants must *mean* in the verifier's file what they mean in
    //    the public environment.
    //
    //    Check 8 only establishes that the names are public. If the verifier wrote their
    //    proposition against a different environment — their own `MyClaim := hard`, while
    //    public.bin holds `MyClaim := True` — the digests still agree, and they would read
    //    a proof of something else as a proof of what they wrote.
    //
    //    Every declaration the two files share is compared: kind, structural name,
    //    universe parameters, type, and defining value. Exports are closed, so comparing
    //    the shared declarations covers the whole dependency closure of the statement.
    //    What is left over belongs to the verifier's own file — the proposition itself,
    //    whose proof is `sorry`.
    let public_fp = audit::fingerprints(&public_ef);
    let expected_fp = audit::fingerprints(&ef);
    let mut shared = 0usize;
    let mut differing: Vec<String> = Vec::new();
    ef.with_ctx(|ctx| {
        let name_of: Vec<(audit::NameKey, String)> = ef
            .declars
            .values()
            .map(|d| (audit::name_key(ctx, d.info().name), stmt::name_to_string(ctx, d.info().name)))
            .collect();
        for (key, fp) in &expected_fp {
            if let Ok(i) = public_fp.binary_search_by(|(k, _)| k.cmp(key)) {
                shared += 1;
                if public_fp[i].1 != *fp {
                    let n = name_of
                        .iter()
                        .find(|(k, _)| k == key)
                        .map(|(_, n)| n.clone())
                        .unwrap_or_else(|| "<unnamed>".to_string());
                    differing.push(n);
                }
            }
        }
    });
    if !differing.is_empty() {
        differing.sort();
        differing.truncate(12);
        fail(&format!(
            "{expected_path} and the public environment disagree about what these mean: {differing:?}\n   \
             The proof is about the public environment's version. Write the proposition against\n   \
             the same environment as public.bin (same toolchain and library revision)."
        ));
    }
    println!("✅ 9. the {shared} declarations shared with {expected_path} are identical, so the statement means the same in both");

    println!();
    if certified {
        println!("Verified. {n_declars} declarations type-check (including the prelude).");
    } else {
        println!("Verified. {n_declars} declarations on the theorem side type-check.");
        println!("The prelude was not type-checked; it was accepted as an assumption.");
    }
    println!("The proof term was never seen.");
}
