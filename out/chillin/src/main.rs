mod externs;
// The checked-in generated snapshot predates the current `tc` lint-clean
// output and cannot be regenerated (the input executable is not in-tree).
#[allow(
    clippy::double_parens,
    clippy::eq_op,
    clippy::large_const_arrays,
    clippy::manual_swap,
    clippy::unnecessary_cast
)]
mod generated;

fn main() {
    winapi::run(&generated::EXEDATA);
}
