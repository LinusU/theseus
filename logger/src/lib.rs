struct Logger {}
impl log::Log for Logger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        // Graphics libraries narrate every shader and adapter they touch;
        // only their problems are of interest here.
        let chatty = ["wgpu", "naga"]
            .iter()
            .any(|prefix| metadata.target().starts_with(prefix));
        !chatty || metadata.level() <= log::Level::Warn
    }

    #[cfg(not(target_family = "wasm"))]
    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        use colored::Colorize;
        use log::Level::*;
        let level = match record.level() {
            Error => format!("{:<5}", record.level()).red(),
            Warn => format!("{:<5}", record.level()).yellow(),
            Info => format!("{:<5}", record.level()).cyan(),
            Debug => format!("{:<5}", record.level()).purple(),
            Trace => format!("{:<5}", record.level()).normal(),
        };
        println!(
            "{} {}:{} {}",
            level,
            record.file().unwrap_or("?"),
            record.line().unwrap_or(0),
            record.args()
        );
    }

    #[cfg(target_family = "wasm")]
    fn log(&self, record: &log::Record) {
        let s: wasm_bindgen::JsValue = format!(
            "{}:{} {}",
            record.file().unwrap_or("?"),
            record.line().unwrap_or(0),
            record.args()
        )
        .into();
        match record.level() {
            log::Level::Error => web_sys::console::error_1(&s),
            log::Level::Warn => web_sys::console::warn_1(&s),
            log::Level::Info => web_sys::console::log_1(&s),
            log::Level::Debug | log::Level::Trace => web_sys::console::debug_1(&s),
        }
    }

    fn flush(&self) {}
}

static LOGGER: Logger = Logger {};

pub fn init() {
    log::set_logger(&LOGGER).unwrap();
    log::set_max_level(log::LevelFilter::Debug);
}
