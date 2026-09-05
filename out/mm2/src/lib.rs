#[cfg(target_family = "wasm")]
use wasm_bindgen::prelude::*;

mod externs;
// The ignored generated snapshot may predate the current `tc` lint-clean
// output; keep clippy green on snapshots that cannot be regenerated.
#[allow(
    clippy::double_parens,
    clippy::large_const_arrays,
    clippy::manual_swap,
    clippy::unnecessary_cast
)]
mod generated;

#[cfg_attr(target_family = "wasm", wasm_bindgen)]
pub fn main() {
    winapi::run(&generated::EXEDATA);
}
