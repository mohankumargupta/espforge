//! `AnswerArgs`: the answers a user can give on the command line, shared by
//! `build` and `new` (flattened into both). A flag always wins over the
//! `.espf`; when it actually changes something a `note:` is printed.
//!
//! This module is only the flag surface: the spellings, the value enums and
//! the conversion into `espforge_core::answers::Overrides`. Applying an
//! override — which value wins, when a note is owed, what an unknown chip
//! means — is library behaviour.

use clap::{Args, ValueEnum};
use espforge_core::answers::Overrides;
use espforge_model::{Console, Runtime};

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

impl From<RuntimeArg> for Runtime {
    fn from(r: RuntimeArg) -> Self {
        match r {
            RuntimeArg::Blocking => Runtime::Blocking,
            RuntimeArg::Embassy => Runtime::Embassy,
        }
    }
}

impl From<ConsoleArg> for Console {
    fn from(c: ConsoleArg) -> Self {
        match c {
            ConsoleArg::Uart => Console::Uart,
            ConsoleArg::JtagSerial => Console::JtagSerial,
        }
    }
}

impl AnswerArgs {
    /// The flags as library values.
    pub fn overrides(&self) -> Overrides {
        Overrides {
            name: self.name.clone(),
            chip: self.chip.clone(),
            runtime: self.runtime.map(Runtime::from),
            console: self.console.map(Console::from),
            defaults: self.defaults,
            options: self.options.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_flag_reaches_the_override_struct() {
        let args = AnswerArgs {
            name: Some("hello".into()),
            chip: Some("esp32s3".into()),
            runtime: Some(RuntimeArg::Embassy),
            console: Some(ConsoleArg::JtagSerial),
            defaults: Some(true),
            options: vec!["wifi=true".into()],
        };
        let o = args.overrides();
        assert_eq!(o.name.as_deref(), Some("hello"));
        assert_eq!(o.chip.as_deref(), Some("esp32s3"));
        assert_eq!(o.runtime, Some(Runtime::Embassy));
        assert_eq!(o.console, Some(Console::JtagSerial));
        assert_eq!(o.defaults, Some(true));
        assert_eq!(o.options, ["wifi=true"]);
    }

    #[test]
    fn no_flags_is_an_empty_override() {
        let o = AnswerArgs::default().overrides();
        assert!(o.name.is_none());
        assert!(o.chip.is_none());
        assert!(o.runtime.is_none());
        assert!(o.console.is_none());
        assert!(o.defaults.is_none());
        assert!(o.options.is_empty());
    }
}
