//! Placeholder:
//! ```ignore
//! Doc comment example
//! ```
#![allow(clippy::too_many_arguments)]
#![deny(clippy::cast_possible_truncation)]

pub mod debug_printer;
pub mod env;
pub mod expr;
pub mod inductive;
pub mod level;
pub mod name;
pub mod parser;
pub mod pretty_printer;
pub mod quot;
pub mod tc;
#[cfg(test)]
mod tests;
/// 型検査の操作回数を数える（`touch_trace` feature が有効なときだけ）。
///
/// feature が無効でも呼び出し側がそのまま書けるよう、マクロ自体は常に定義する。
#[macro_export]
macro_rules! count_op {
    ($name:ident) => {
        #[cfg(feature = "touch_trace")]
        $crate::touch_trace::ops::bump(&$crate::touch_trace::ops::$name);
    };
}

#[cfg(feature = "touch_trace")]
pub mod touch_trace;
pub mod unique_hasher;
pub mod util;

pub(crate) const STACK_SIZE: usize = 16_777_216;
