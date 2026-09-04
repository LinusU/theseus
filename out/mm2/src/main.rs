pub fn main() {
    if let Some(dir) = std::env::args().nth(1) {
        std::env::set_current_dir(&dir).unwrap();
    }
    mm2::main();
}
