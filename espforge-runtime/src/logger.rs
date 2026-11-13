//! A target-aware `log` backend, configured by `<logger>`.
//!
//! `esp-println`'s `ESP_LOG` parser reads one compile-time environment string.
//! This logger does the same matching in Rust: the generated project declares
//! the rules (`<logger drivers="debug" />` becomes a set of target prefixes)
//! and hands them to [`FilteredLogger`], whose `log::Log` implementation drops
//! a record unless its level is at or below the level of the first rule whose
//! prefix matches its target.
//!
//! The logger itself is a `static` the **generated** code declares, not one
//! this crate hides: the runtime's storage rule applies here too, and a
//! project's `<logger>` is what decides the table.

use log::{LevelFilter, Log, Metadata, Record};

/// One target filter: a module prefix and the level it allows.
///
/// The prefix matches at a module boundary: `esp_radio` covers
/// `esp_radio::wifi` but not `esp_radio_x`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FilterRule {
    pub prefix: &'static str,
    pub level: LevelFilter,
}

/// The filtered logger the generated entry point installs.
#[derive(Debug)]
pub struct FilteredLogger {
    /// The level for every target no rule matches.
    default_level: LevelFilter,
    /// Rules in match order: the first prefix that matches a target wins.
    rules: &'static [FilterRule],
}

impl FilteredLogger {
    /// Build the logger. `const` so the generated code can declare it in a
    /// `static` without a constructor call at boot.
    pub const fn new(default_level: LevelFilter, rules: &'static [FilterRule]) -> Self {
        Self {
            default_level,
            rules,
        }
    }

    /// The level that applies to `target`: the first matching rule's, or the
    /// default.
    pub fn level_for(&self, target: &str) -> LevelFilter {
        for rule in self.rules {
            if prefix_matches(rule.prefix, target) {
                return rule.level;
            }
        }
        self.default_level
    }

    /// The most verbose level any target allows.
    ///
    /// The `log` facade drops a record above its global maximum *before* it
    /// reaches this backend, so the maximum has to be the highest rule, not
    /// the default: a `drivers="debug"` rule would otherwise never see a
    /// debug record.
    pub fn max_level(&self) -> LevelFilter {
        let mut max = self.default_level;
        for rule in self.rules {
            if rank(rule.level) > rank(max) {
                max = rule.level;
            }
        }
        max
    }
}

impl Log for FilteredLogger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        allowed(self.level_for(metadata.target()), metadata.level())
    }

    fn log(&self, record: &Record<'_>) {
        if self.enabled(record.metadata()) {
            emit(record);
        }
    }

    fn flush(&self) {}
}

/// Install `logger` as the global `log` backend and set its maximum level.
///
/// The generated entry point is the only caller; `log` accepts one backend per
/// process, so a second call (an app that installs its own) keeps the first,
/// which is the one the project asked for.
pub fn init(logger: &'static FilteredLogger) {
    log::set_max_level(logger.max_level());
    let _ = log::set_logger(logger);
}

/// Whether a record at `level` is inside `filter`.
fn allowed(filter: LevelFilter, level: log::Level) -> bool {
    let max = match filter {
        LevelFilter::Off => return false,
        LevelFilter::Error => log::Level::Error,
        LevelFilter::Warn => log::Level::Warn,
        LevelFilter::Info => log::Level::Info,
        LevelFilter::Debug => log::Level::Debug,
        LevelFilter::Trace => log::Level::Trace,
    };
    level <= max
}

/// Verbosity as a number, so `Off` sorts below every level and the maximum
/// does not depend on the `LevelFilter` enum's declared order.
fn rank(filter: LevelFilter) -> u8 {
    match filter {
        LevelFilter::Off => 0,
        LevelFilter::Error => 1,
        LevelFilter::Warn => 2,
        LevelFilter::Info => 3,
        LevelFilter::Debug => 4,
        LevelFilter::Trace => 5,
    }
}

/// Whether `prefix` names `target` or one of its modules.
fn prefix_matches(prefix: &str, target: &str) -> bool {
    match target.strip_prefix(prefix) {
        Some(rest) => rest.is_empty() || rest.starts_with(':'),
        None => false,
    }
}

/// Write one allowed record. The format matches esp-println's own logger, so a
/// project switching between the two reads the same lines.
#[cfg(any(target_arch = "riscv32", target_arch = "xtensa"))]
fn emit(record: &Record<'_>) {
    esp_println::println!("{} - {}", record.level(), record.args());
}

/// A host build has no console; the filtering is what the tests exercise.
#[cfg(not(any(target_arch = "riscv32", target_arch = "xtensa")))]
fn emit(record: &Record<'_>) {
    let _ = record;
}

#[cfg(test)]
mod tests {
    use super::*;
    use log::Level;

    const RULES: &[FilterRule] = &[
        FilterRule {
            prefix: "esp_radio",
            level: LevelFilter::Error,
        },
        FilterRule {
            prefix: "espforge_runtime::hw::wifi",
            level: LevelFilter::Off,
        },
        FilterRule {
            prefix: "espforge_drivers",
            level: LevelFilter::Debug,
        },
    ];

    fn logger() -> FilteredLogger {
        FilteredLogger::new(LevelFilter::Warn, RULES)
    }

    fn metadata(level: Level, target: &'static str) -> Metadata<'static> {
        let mut builder = log::MetadataBuilder::new();
        builder.level(level).target(target);
        builder.build()
    }

    #[test]
    fn an_unmatched_target_takes_the_default_level() {
        assert_eq!(logger().level_for("my_app::app"), LevelFilter::Warn);
    }

    #[test]
    fn a_prefix_matches_at_a_module_boundary() {
        let logger = logger();
        assert_eq!(logger.level_for("esp_radio"), LevelFilter::Error);
        assert_eq!(logger.level_for("esp_radio::wifi"), LevelFilter::Error);
        assert_eq!(logger.level_for("esp_radio_x"), LevelFilter::Warn);
    }

    #[test]
    fn the_first_matching_rule_wins() {
        // A module path inside a wider prefix takes the more specific rule
        // only when that rule comes first, which is why the emitter writes the
        // Wi-Fi rule before any crate-wide one.
        assert_eq!(
            logger().level_for("espforge_runtime::hw::wifi::join"),
            LevelFilter::Off
        );
    }

    #[test]
    fn the_global_maximum_is_the_most_verbose_rule() {
        assert_eq!(logger().max_level(), LevelFilter::Debug);
        assert_eq!(
            FilteredLogger::new(LevelFilter::Off, &[]).max_level(),
            LevelFilter::Off
        );
        assert_eq!(
            FilteredLogger::new(LevelFilter::Off, RULES).max_level(),
            LevelFilter::Debug
        );
    }

    #[test]
    fn a_record_is_enabled_only_at_or_below_its_targets_level() {
        let logger = logger();
        assert!(log::Log::enabled(
            &logger,
            &metadata(Level::Error, "esp_radio::wifi")
        ));
        assert!(!log::Log::enabled(
            &logger,
            &metadata(Level::Warn, "esp_radio::wifi")
        ));
        assert!(log::Log::enabled(
            &logger,
            &metadata(Level::Debug, "espforge_drivers::ssd1306")
        ));
        assert!(!log::Log::enabled(
            &logger,
            &metadata(Level::Error, "espforge_runtime::hw::wifi")
        ));
        assert!(!log::Log::enabled(
            &logger,
            &metadata(Level::Info, "my_app::app")
        ));
    }
}
