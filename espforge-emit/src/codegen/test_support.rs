//! Fixtures shared by the codegen backends' unit tests.
//!
//! `wifi` and `embassy` are derived flags: they follow `# services` and the
//! runtime, and setting them by hand is a resolver error. So the fixture takes
//! the runtime and the service list, not an option flag for them.

use espforge_model::{Console, Meta, Runtime};
use espforge_options::{Answers, DerivedInputs, ResolvedOptions};

use super::chip::Chip;

pub struct Case {
    pub meta: Meta,
    pub opts: ResolvedOptions,
}

/// A resolved project: `hello world` on `chip`, with `flags` applied.
///
/// `defaults` is the `<options defaults=...>` switch; `services` are the names
/// in `# services` (the only way to turn `wifi` on).
pub fn case(
    chip: &str,
    runtime: Runtime,
    defaults: bool,
    flags: &[(&str, &str)],
    services: &[&str],
) -> Case {
    let meta = Meta {
        runtime,
        ..meta(chip)
    };
    let mut answers = Answers::empty(0..0);
    answers.defaults = defaults;
    for (name, value) in flags {
        answers
            .apply_flag(espforge_options::embedded_schema(), name, value)
            .unwrap_or_else(|e| panic!("option `{name}={value}`: {e:?}"));
    }
    let derived = DerivedInputs {
        services: services
            .iter()
            .map(|name| (name.to_string(), 0..1))
            .collect(),
        console_span: None,
    };
    let opts = espforge_options::resolve(&meta, &answers, &derived)
        .unwrap_or_else(|e| panic!("resolving the fixture: {e:?}"));
    Case { meta, opts }
}

pub fn chip(name: &str) -> Chip {
    Chip::resolve(name).unwrap()
}

/// The project settings every fixture starts from.
pub fn meta(chip: &str) -> Meta {
    Meta {
        name: "hello world".into(),
        description: Some("demo".into()),
        chip: chip.into(),
        runtime: Runtime::Blocking,
        console: Console::Uart,
    }
}
