struct Logger {
    /// Parsed `RUST_LOG` directives, populated by `init`. Each entry is a
    /// (target prefix, level) pair; an empty prefix is a global default.
    filters: std::sync::OnceLock<Vec<(String, log::LevelFilter)>>,
}

impl Logger {
    /// env_logger-style filtering: a comma-separated list of `level` or
    /// `target=level` directives. A bare `level` sets the global default;
    /// `target=level` applies to records whose target starts with that path.
    /// The longest matching target prefix wins, and a target matched by no
    /// directive is silenced.
    fn enabled_level(&self, metadata: &log::Metadata) -> Option<log::LevelFilter> {
        let filters = self.filters.get()?;
        let mut best: Option<&(String, log::LevelFilter)> = None;
        for filter in filters {
            if metadata.target().starts_with(filter.0.as_str())
                && best.is_none_or(|b| filter.0.len() >= b.0.len())
            {
                best = Some(filter);
            }
        }
        best.map(|(_, level)| *level)
    }
}

impl log::Log for Logger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        // Before init() runs there is no spec; keep every record.
        self.enabled_level(metadata)
            .is_some_and(|level| metadata.level() <= level)
    }

    #[cfg(not(target_family = "wasm"))]
    fn log(&self, record: &log::Record) {
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

static LOGGER: Logger = Logger {
    filters: std::sync::OnceLock::new(),
};

pub fn init() {
    let mut filters = Vec::new();
    if let Ok(spec) = std::env::var("RUST_LOG") {
        for part in spec.split(',') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            let (target, level_str) = match part.split_once('=') {
                Some((target, level)) => (target.trim(), level.trim()),
                None => ("", part),
            };
            match level_str.parse::<log::LevelFilter>() {
                Ok(level) => filters.push((target.to_string(), level)),
                Err(_) => eprintln!("logger: ignoring bad RUST_LOG directive {part:?}"),
            }
        }
    }
    // No usable spec: the historical default is Debug for every target.
    if filters.is_empty() {
        filters.push((String::new(), log::LevelFilter::Debug));
    }
    // The macros gate on this static maximum first, so it must cover the
    // most verbose directive or enabled() never sees those records.
    let max = filters
        .iter()
        .map(|(_, level)| *level)
        .max()
        .unwrap_or(log::LevelFilter::Debug);
    let _ = LOGGER.filters.set(filters);
    log::set_logger(&LOGGER).unwrap();
    log::set_max_level(max);
}
