pub fn main() {
    if let Some(dir) = std::env::args().nth(1)
        && let Err(err) = std::env::set_current_dir(&dir)
    {
        eprintln!("cannot cd to {dir}: {err}");
        std::process::exit(1);
    }
    mm2::main();
}
