#[cfg(target_family = "wasm")]
use wasm_bindgen::prelude::*;

// The checked-in generated snapshot predates the current `tc` lint-clean
// output and cannot be regenerated (the input executable is not in-tree).
#[allow(clippy::double_parens, clippy::large_const_arrays)]
mod generated;

#[cfg_attr(target_family = "wasm", wasm_bindgen)]
pub fn main() {
    winapi::run(&generated::EXEDATA);
}
