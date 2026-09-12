// envcheck is built first, in methods-env, so its image ID can be baked into thmcheck.
pub use methods_env::{ENVCHECK_ELF, ENVCHECK_ID};

include!(concat!(env!("OUT_DIR"), "/methods.rs"));
