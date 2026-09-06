#[cfg(target_family = "wasm")]
use wasm_bindgen::prelude::*;

mod externs;
// The ignored generated snapshot is produced by the current `tc` clippy-clean
// output, so no extra allows are needed.
mod generated;

#[cfg_attr(target_family = "wasm", wasm_bindgen)]
pub fn main() {
    winapi::run(&generated::EXEDATA);
}
