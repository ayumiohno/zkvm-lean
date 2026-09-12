//! A verifier's tool.
//!
//!   stmtdigest <export.ndjson> <theorem>
//!
//! Compute and print the canonical digest of a named declaration's type (its
//! statement) from a public export file.
//!
//! Comparing it against the value in a zk proof's journal is how a verifier confirms
//! the proposition they intended was the one proved. The proof term is never seen.

use std::io::{BufReader, Cursor};

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("usage: stmtdigest <export.ndjson> <theorem>");
    let target = args.next().expect("usage: stmtdigest <export.ndjson> <theorem>");

    let bytes = std::fs::read(&path).expect("cannot read export file");
    let config = stmt::policy::config();

    let (export_file, _) = config
        .to_export_file_from_reader(BufReader::new(Cursor::new(&bytes)))
        .expect("failed to parse export file");

    match stmt::statement_digest(&export_file, &target) {
        Some(d) => println!("{}  {}", stmt::hex(&d), target),
        None => {
            eprintln!("{target} was not found in {path}");
            std::process::exit(1);
        }
    }
}
