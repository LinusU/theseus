// The checked-in generated snapshot predates the current `tc` lint-clean
// output and cannot be regenerated (the input executable is not in-tree).
#[allow(clippy::double_parens, clippy::unnecessary_cast)]
mod generated;

fn main() {
    winapi::run(&generated::EXEDATA);
}
