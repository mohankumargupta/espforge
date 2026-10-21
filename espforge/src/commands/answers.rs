//! `AnswerArgs`: the answers a user can give on the command line, shared by
//! `build` and `new` (flattened into both). A flag always wins over the
//! `.espf`; when it actually changes something a `note:` is printed.

use clap::{Args, ValueEnum};
use espforge_model::{Console, Meta, Runtime};
use espforge_options::{Answers, OptValue, Origin, Schema, chips};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum RuntimeArg {
    Blocking,
    Embassy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ConsoleArg {
    Uart,
    JtagSerial,
}

#[derive(Args, Debug, Clone, Default)]
pub struct AnswerArgs {
    /// Project name (overrides <project name="...">)
    #[arg(long)]
    pub name: Option<String>,
    /// Chip, e.g. esp32c3 (overrides <chip type="...">)
    #[arg(long)]
    pub chip: Option<String>,
    /// Runtime (overrides <runtime type="...">)
    #[arg(long, value_enum)]
    pub runtime: Option<RuntimeArg>,
    /// Console (overrides <console type="...">)
    #[arg(long, value_enum)]
    pub console: Option<ConsoleArg>,
    /// Fill unspecified options from their defaults (true/false)
    #[arg(long, value_name = "BOOL")]
    pub defaults: Option<bool>,
    /// Set an option, repeatable: --option wifi=true --option log=false
    #[arg(long = "option", value_name = "NAME=VALUE")]
    pub options: Vec<String>,
}

fn runtime_name(r: Runtime) -> &'static str {
    match r {
        Runtime::Blocking => "blocking",
        Runtime::Embassy => "embassy",
    }
}

fn console_name(c: Console) -> &'static str {
    match c {
        Console::Uart => "uart",
        Console::JtagSerial => "jtag-serial",
    }
}

/// `<options name="value" />` as the user would have written it.
fn show_option(name: &str, v: &OptValue) -> String {
    match v {
        OptValue::Bool(b) => format!("<options {name}=\"{b}\" />"),
        OptValue::Str(s) => format!("<options {name}=\"{s}\" />"),
        OptValue::List(l) => {
            let items: Vec<String> = l.iter().map(|i| format!("{name}+=\"{i}\"")).collect();
            format!("<options {} />", items.join(" "))
        }
    }
}

impl AnswerArgs {
    /// Override the core settings. Pushes a `note:` per value that changed.
    pub fn apply_core(&self, meta: &mut Meta, notes: &mut Vec<String>) -> Result<(), String> {
        if let Some(n) = &self.name {
            if *n != meta.name {
                notes.push(format!(
                    "note: --name {n} overrides <project name=\"{}\" />",
                    meta.name
                ));
                meta.name = n.clone();
            }
        }
        if let Some(c) = &self.chip {
            if !chips::is_supported_chip(c) {
                return Err(format!(
                    "unknown chip \"{c}\" for --chip (expected one of: {})",
                    chips::supported_chips().join(", ")
                ));
            }
            if *c != meta.chip {
                notes.push(format!(
                    "note: --chip {c} overrides <chip type=\"{}\" />",
                    meta.chip
                ));
                meta.chip = c.clone();
            }
        }
        if let Some(r) = self.runtime {
            let r = match r {
                RuntimeArg::Blocking => Runtime::Blocking,
                RuntimeArg::Embassy => Runtime::Embassy,
            };
            if r != meta.runtime {
                notes.push(format!(
                    "note: --runtime {} overrides <runtime type=\"{}\" />",
                    runtime_name(r),
                    runtime_name(meta.runtime)
                ));
                meta.runtime = r;
            }
        }
        if let Some(c) = self.console {
            let c = match c {
                ConsoleArg::Uart => Console::Uart,
                ConsoleArg::JtagSerial => Console::JtagSerial,
            };
            if c != meta.console {
                notes.push(format!(
                    "note: --console {} overrides <console type=\"{}\" />",
                    console_name(c),
                    console_name(meta.console)
                ));
                meta.console = c;
            }
        }
        Ok(())
    }

    /// Apply `--defaults` and `--option NAME=VALUE`. Pushes a `note:` when an
    /// option written in the `.espf` is changed to a different value.
    pub fn apply_options(
        &self,
        schema: &Schema,
        answers: &mut Answers,
        notes: &mut Vec<String>,
    ) -> Result<(), String> {
        if let Some(d) = self.defaults {
            answers.defaults = d;
        }
        for raw in &self.options {
            let (name, value) = raw
                .split_once('=')
                .ok_or_else(|| format!("--option expects NAME=VALUE, got \"{raw}\""))?;
            if let Some(prev) = answers.apply_flag(schema, name, value)? {
                if matches!(prev.origin, Origin::Espf(_))
                    && prev.value != answers.values[name].value
                {
                    notes.push(format!(
                        "note: --option {name}={value} overrides {}",
                        show_option(name, &prev.value)
                    ));
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use espforge_options::embedded_schema;

    fn meta() -> Meta {
        Meta {
            name: "hello".into(),
            description: None,
            chip: "esp32c3".into(),
            runtime: Runtime::Blocking,
            console: Console::Uart,
        }
    }

    #[test]
    fn chip_flag_overrides_with_a_note() {
        let args = AnswerArgs {
            chip: Some("esp32s3".into()),
            ..Default::default()
        };
        let (mut m, mut notes) = (meta(), vec![]);
        args.apply_core(&mut m, &mut notes).unwrap();
        assert_eq!(m.chip, "esp32s3");
        assert_eq!(
            notes,
            ["note: --chip esp32s3 overrides <chip type=\"esp32c3\" />"]
        );
    }

    #[test]
    fn a_flag_equal_to_the_file_is_silent() {
        let args = AnswerArgs {
            chip: Some("esp32c3".into()),
            runtime: Some(RuntimeArg::Blocking),
            console: Some(ConsoleArg::Uart),
            name: Some("hello".into()),
            ..Default::default()
        };
        let (mut m, mut notes) = (meta(), vec![]);
        args.apply_core(&mut m, &mut notes).unwrap();
        assert!(notes.is_empty());
    }

    #[test]
    fn runtime_console_and_name_notes() {
        let args = AnswerArgs {
            runtime: Some(RuntimeArg::Embassy),
            console: Some(ConsoleArg::JtagSerial),
            name: Some("other".into()),
            ..Default::default()
        };
        let (mut m, mut notes) = (meta(), vec![]);
        args.apply_core(&mut m, &mut notes).unwrap();
        assert_eq!(m.runtime, Runtime::Embassy);
        assert_eq!(m.console, Console::JtagSerial);
        assert!(notes.contains(
            &"note: --runtime embassy overrides <runtime type=\"blocking\" />".to_string()
        ));
        assert!(notes.contains(
            &"note: --console jtag-serial overrides <console type=\"uart\" />".to_string()
        ));
        assert!(
            notes.contains(&"note: --name other overrides <project name=\"hello\" />".to_string())
        );
    }

    #[test]
    fn unknown_chip_flag_is_an_error() {
        let args = AnswerArgs {
            chip: Some("esp99".into()),
            ..Default::default()
        };
        assert!(
            args.apply_core(&mut meta(), &mut vec![])
                .unwrap_err()
                .contains("unknown chip")
        );
    }

    #[test]
    fn option_flag_overrides_the_espf_with_a_note() {
        let schema = embedded_schema();
        let mut answers = Answers::empty(0..0);
        answers.apply_flag(schema, "alloc", "true").unwrap();
        // pretend that came from the file
        answers.values.get_mut("alloc").unwrap().origin = Origin::Espf(0..1);
        let args = AnswerArgs {
            options: vec!["alloc=false".into()],
            ..Default::default()
        };
        let mut notes = vec![];
        args.apply_options(schema, &mut answers, &mut notes)
            .unwrap();
        assert_eq!(answers.values["alloc"].value, OptValue::Bool(false));
        assert_eq!(
            notes,
            ["note: --option alloc=false overrides <options alloc=\"true\" />"]
        );
    }

    #[test]
    fn bad_option_flags_are_errors() {
        let schema = embedded_schema();
        let mut a = Answers::empty(0..0);
        let bad = |s: &str| AnswerArgs {
            options: vec![s.into()],
            ..Default::default()
        };
        assert!(
            bad("alloc")
                .apply_options(schema, &mut a, &mut vec![])
                .is_err()
        );
        assert!(
            bad("nope=1")
                .apply_options(schema, &mut a, &mut vec![])
                .is_err()
        );
        assert!(
            bad("alloc=maybe")
                .apply_options(schema, &mut a, &mut vec![])
                .is_err()
        );
    }

    #[test]
    fn defaults_flag_is_applied() {
        let mut a = Answers::empty(0..0);
        let args = AnswerArgs {
            defaults: Some(false),
            ..Default::default()
        };
        args.apply_options(embedded_schema(), &mut a, &mut vec![])
            .unwrap();
        assert!(!a.defaults);
    }
}
