struct Logger {
    /// Parsed `RUST_LOG` directives, populated by `init`. Each entry is a
    /// (target prefix, level) pair; an empty prefix is a global default.
    filters: std::sync::OnceLock<Vec<(String, log::LevelFilter)>>,
}

impl Logger {
    /// env_logger-style filtering: a comma-separated list of `level`,
    /// `target`, or `target=level` directives. A bare `level` sets the
    /// global default; a bare `target` enables every level for that target;
    /// `target=level` applies to records whose target starts with that
    /// path. The longest matching target prefix wins, and a target matched
    /// by no directive is silenced.
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

/// Parse one `RUST_LOG` directive into a (target prefix, level) pair.
/// Returns `None` for a malformed `target=level` directive.
fn parse_directive(part: &str) -> Option<(String, log::LevelFilter)> {
    match part.split_once('=') {
        Some((target, level)) => level
            .trim()
            .parse::<log::LevelFilter>()
            .ok()
            .map(|level| (target.trim().to_string(), level)),
        None => match part.parse::<log::LevelFilter>() {
            // A bare level is the global default.
            Ok(level) => Some((String::new(), level)),
            // env_logger-style: a bare target name enables that target at
            // the most verbose level.
            Err(_) => Some((part.to_string(), log::LevelFilter::Trace)),
        },
    }
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
            match parse_directive(part) {
                Some(filter) => filters.push(filter),
                None => eprintln!("logger: ignoring bad RUST_LOG directive {part:?}"),
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

#[cfg(test)]
mod tests {
    use super::{Logger, parse_directive};
    use log::{Level, LevelFilter, Log, Metadata};

    fn logger(directives: &[(&str, LevelFilter)]) -> Logger {
        let logger = Logger {
            filters: std::sync::OnceLock::new(),
        };
        logger
            .filters
            .set(
                directives
                    .iter()
                    .map(|(target, level)| (target.to_string(), *level))
                    .collect(),
            )
            .unwrap();
        logger
    }

    fn meta(target: &'static str, level: Level) -> Metadata<'static> {
        Metadata::builder().target(target).level(level).build()
    }

    #[test]
    fn bare_target_enables_all_levels_for_that_target() {
        // env_logger treats `RUST_LOG=winapi` as `winapi=trace`.
        assert_eq!(
            parse_directive("winapi"),
            Some(("winapi".to_string(), LevelFilter::Trace))
        );
        assert_eq!(
            parse_directive("warn"),
            Some((String::new(), LevelFilter::Warn))
        );
        assert_eq!(parse_directive("winapi=bogus"), None);

        let logger = logger(&[("winapi", LevelFilter::Trace)]);
        assert_eq!(
            logger.enabled_level(&meta("winapi::dsound", Level::Trace)),
            Some(LevelFilter::Trace)
        );
        // A target matched by no directive is silenced.
        assert_eq!(logger.enabled_level(&meta("host::sdl", Level::Error)), None);
    }

    #[test]
    fn longest_matching_prefix_wins() {
        let logger = logger(&[
            ("", LevelFilter::Warn),
            ("winapi", LevelFilter::Error),
            ("winapi::dsound", LevelFilter::Trace),
        ]);
        assert_eq!(
            logger.enabled_level(&meta("winapi::dsound", Level::Trace)),
            Some(LevelFilter::Trace)
        );
        assert_eq!(
            logger.enabled_level(&meta("winapi::kernel32", Level::Warn)),
            Some(LevelFilter::Error)
        );
        assert_eq!(
            logger.enabled_level(&meta("host", Level::Warn)),
            Some(LevelFilter::Warn)
        );
    }

    #[test]
    fn off_directive_silences_everything() {
        let logger = logger(&[("", LevelFilter::Off)]);
        // The filter matches (so enabled_level reports it), but no record
        // passes the `enabled` comparison against Off.
        assert_eq!(
            logger.enabled_level(&meta("x", Level::Error)),
            Some(LevelFilter::Off)
        );
        assert!(!logger.enabled(&meta("x", Level::Error)));
        assert!(!logger.enabled(&meta("x", Level::Trace)));
    }
}
