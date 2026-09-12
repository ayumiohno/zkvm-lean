//! Confirm on the host that the binary format yields the same result as NDJSON.
use nanoda_lib::parser::{ndjson_to_binary, parse_export_file_binary};
use std::io::{BufReader, Cursor};

fn main() {
    let path = std::env::args().nth(1).expect("usage: checkbin <export.ndjson>");
    let ndjson = std::fs::read(&path).unwrap();

    let (a, _) = stmt::policy::config().to_export_file_from_reader(BufReader::new(Cursor::new(&ndjson))).unwrap();
    let n_json = a.declars.len();
    let d_json = stmt::statement_digest(&a, "Preimage.knows_preimage");

    let bin = ndjson_to_binary(BufReader::new(Cursor::new(&ndjson))).unwrap();
    let (b, _) = parse_export_file_binary(&bin, stmt::policy::config()).unwrap();
    let n_bin = b.declars.len();
    let d_bin = stmt::statement_digest(&b, "Preimage.knows_preimage");

    println!("declarations      json={n_json} bin={n_bin}  {}", if n_json == n_bin { "match" } else { "MISMATCH" });
    println!("statement digest  {}", if d_json == d_bin { "match" } else { "MISMATCH" });
    if let Some(d) = d_bin { println!("                  {}", stmt::hex(&d)); }
    b.check_all_declars();
    println!("the binary path also type-checks");
    assert_eq!(n_json, n_bin);
    assert_eq!(d_json, d_bin);
}
