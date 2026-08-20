use std::{
    io::Write,
    sync::Once,
    time::{SystemTime, UNIX_EPOCH},
};

struct StderrLogger;

static LOGGER: StderrLogger = StderrLogger;
static INIT: Once = Once::new();

impl log::Log for StderrLogger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::max_level()
    }

    fn log(&self, record: &log::Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis())
            .unwrap_or_default();
        let _ = writeln!(
            std::io::stderr().lock(),
            "{timestamp_ms} {:<5} {}: {}",
            record.level(),
            record.target(),
            record.args()
        );
    }

    fn flush(&self) {
        let _ = std::io::stderr().lock().flush();
    }
}

pub(crate) fn init() {
    INIT.call_once(|| {
        if log::set_logger(&LOGGER).is_ok() {
            log::set_max_level(configured_level());
        }
    });
}

fn configured_level() -> log::LevelFilter {
    std::env::var("MEDIA_RIP_LOG")
        .ok()
        .or_else(|| std::env::var("RUST_LOG").ok())
        .as_deref()
        .and_then(parse_level)
        .unwrap_or(if cfg!(debug_assertions) {
            log::LevelFilter::Debug
        } else {
            log::LevelFilter::Info
        })
}

fn parse_level(value: &str) -> Option<log::LevelFilter> {
    // Accept both a plain level and the common `module=level` RUST_LOG form.
    let value = value
        .rsplit([',', '='])
        .next()
        .unwrap_or(value)
        .trim()
        .to_ascii_lowercase();
    match value.as_str() {
        "off" => Some(log::LevelFilter::Off),
        "error" => Some(log::LevelFilter::Error),
        "warn" | "warning" => Some(log::LevelFilter::Warn),
        "info" => Some(log::LevelFilter::Info),
        "debug" => Some(log::LevelFilter::Debug),
        "trace" => Some(log::LevelFilter::Trace),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::parse_level;

    #[test]
    fn parses_plain_and_module_style_log_levels() {
        assert_eq!(parse_level("debug"), Some(log::LevelFilter::Debug));
        assert_eq!(
            parse_level("media_rip_engine=trace"),
            Some(log::LevelFilter::Trace)
        );
        assert_eq!(parse_level("not-a-level"), None);
    }
}
