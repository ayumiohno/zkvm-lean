//! Break down parse cost.
use methods::{PARSEPROBE_ELF, PARSEPROBE_ID};
use risc0_zkvm::{default_executor, ExecutorEnv};

fn main() {
    let path = std::env::args().nth(1).expect("usage: parseprobe <export.ndjson>");
    let bytes = std::fs::read(&path).expect("cannot read");
    let mut b = ExecutorEnv::builder();
    b.write(&(bytes.len() as u32)).unwrap();
    let mut padded = bytes.clone();
    padded.resize(bytes.len().div_ceil(4) * 4, 0);
    b.write_slice(bytemuck::cast_slice::<u8, u32>(&padded));
    let s = default_executor().execute(b.build().unwrap(), PARSEPROBE_ELF).unwrap();
    println!("total cycles = {} (image {:?})", s.cycles(), &PARSEPROBE_ID[..2]);
}
