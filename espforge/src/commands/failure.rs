//! One failure policy for every command: why a command stopped, how it is
//! reported, and the exit code it earns.
//!
//! Exit codes: 0 success, 1 the user's input is at fault (already explained),
//! 2 an internal or IO failure. Deciding this from typed failures keeps every
//! command from inventing its own policy.

use std::process::ExitCode;

use espforge_core::project::ProjectError;
use espforge_core::source::Source;

use super::report;

/// Why a command stopped.
#[derive(Debug)]
pub enum Failure {
    /// The user's input is at fault; the report is already printed.
    User,
    /// Something broke inside espforge or the OS, with its full chain.
    Internal(String),
}

/// Print `❌ msg` and return the user-error marker.
pub fn user(msg: impl std::fmt::Display) -> Failure {
    eprintln!("❌ {msg}");
    Failure::User
}

/// A core failure with an exit code: located spec problems render as
/// snippets, user problems as the one-line `❌`, ours as exit 2.
pub fn failed(error: ProjectError, source: &Source) -> Failure {
    match error {
        ProjectError::Spec(diagnostics) => {
            report::diagnostics(&source.name, &source.text, &diagnostics);
            Failure::User
        }
        ProjectError::User(message) => user(message),
        ProjectError::Internal(e) => Failure::Internal(format!("{e:#}")),
    }
}

/// The exit code a command earns: 0, 1 or 2.
pub fn exit_code(result: Result<(), Failure>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(Failure::User) => ExitCode::from(1),
        Err(Failure::Internal(message)) => {
            eprintln!("❌ {message}");
            ExitCode::from(2)
        }
    }
}
