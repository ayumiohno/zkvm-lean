//! バイト列の受け渡し。
//!
//! risc0 の serde は `Vec<u8>` を 1 バイトにつき u32 1 語で符号化するため、
//! 数百 KB の export file を渡すだけで数千万 cycle かかる。
//! 長さ + 生の語列で渡せば 1/4 のデータ量で済み、serde の処理も要らない。

use risc0_zkvm::guest::env;

pub fn read_bytes() -> Vec<u8> {
    let len: u32 = env::read();
    let mut words = vec![0u32; len.div_ceil(4) as usize];
    env::read_slice(&mut words);
    let mut bytes: Vec<u8> = bytemuck::cast_slice(&words).to_vec();
    bytes.truncate(len as usize);
    bytes
}
