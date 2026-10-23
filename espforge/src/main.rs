mod commands;
mod render;
mod source;

use clap::{Args, CommandFactory, Parser, Subcommand};
use commands::build::{BuildArgs, Stage};
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(name = "espforge", version, about = "ESPF runtime pipeline tool")]
struct Cli {
    /// Run as if started in DIR (like `cargo -C`)
    #[arg(short = 'C', long = "directory", global = true, value_name = "DIR")]
    directory: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Scaffold a new ESPF project (asks for what you do not give as flags).
    #[command(
        alias = "init",
        after_help = "Examples:\n  espforge new\n  espforge new my_proj\n  espforge new --yes --chip esp32c3 --runtime blocking my_proj"
    )]
    New(commands::new::NewArgs),
    /// Validate an ESPF file: syntax, options, `$` references, drivers.
    ///
    /// Uses the same pipeline and override flags as `build`.
    #[command(
        alias = "validate",
        after_help = "Examples:\n  espforge check\n  espforge check hello.espf --chip esp32s3\n  espforge check --example 01.Basics/helloworld"
    )]
    Check(commands::check::CheckArgs),
    /// Generate the firmware project, then `cargo build` it.
    ///
    /// Flags override the file; a `note:` says when one changes something.
    #[command(
        after_help = "Examples:\n  espforge build\n  espforge build hello.espf --out build --release\n  espforge build --example 01.Basics/helloworld"
    )]
    Build(BuildCmd),
    /// Generate the firmware project only (no cargo).
    #[command(
        after_help = "Examples:\n  espforge gen\n  espforge gen hello.espf --chip esp32s3 --option alloc=false --write-spec effective.espf"
    )]
    Gen(BuildArgs),
    /// Turn an embedded ESPF example into an editable project folder.
    ///
    /// Bare `examples` opens the interactive picker (category → example →
    /// destination folder). Subcommands are the non-interactive plumbing
    /// surface:
    ///
    ///   espforge examples categories
    ///   espforge examples list [category]
    ///   espforge examples get <category>/<name> --out <dir>
    #[command(
        after_help = "Examples:\n  espforge examples\n  espforge examples categories\n  espforge examples list\n  espforge examples list 01.Basics\n  espforge examples get 01.Basics/button --out button"
    )]
    Examples {
        #[command(subcommand)]
        command: Option<ExamplesSub>,
    },
    /// Explore what an ESPF file can contain: sections, elements, examples.
    /// A trailing `help` or `examples` is that element's view.
    #[command(
        alias = "ref",
        after_help = "Grammar:\n  espforge espf                        everything, as commands\n  espforge espf overview               the ESPF format in one page\n  espforge espf peripherals            the section and its elements\n  espforge espf peripherals i2c help   the man page for i2c\n  espforge espf i2c                    shorthand: a unique element name\n\nExamples:\n  espforge espf\n  espforge espf overview\n  espforge espf peripherals\n  espforge espf peripherals i2c help\n  espforge espf peripherals i2c examples"
    )]
    Espf(commands::espf::EspfArgs),
    /// Check that prerequisites (toolchain, binaries, env) are met.
    #[command(after_help = "Example:\n  espforge doctor")]
    Doctor,
    /// Print version/build information.
    #[command(after_help = "Example:\n  espforge version")]
    Version,
}

#[derive(Args)]
struct BuildCmd {
    #[command(flatten)]
    args: BuildArgs,
    /// Build the generated project in release mode
    #[arg(long)]
    release: bool,
}

/// Non-interactive (plumbing) subcommands of `examples`: no prompts, plain
/// output, scriptable exit codes.
#[derive(Subcommand)]
enum ExamplesSub {
    /// List the example categories, one per line: `category<TAB>N examples`.
    Categories,
    /// List embedded examples, one per line: `category/name<TAB>title`.
    ///
    /// Without a category, every example of every category is listed.
    List {
        /// Only list examples of this category (e.g. `01.Basics`)
        category: Option<String>,
    },
    /// Copy an embedded example to a new folder (destination never overwritten).
    Get {
        /// The example to copy, as `category/name` (e.g. `01.Basics/button`)
        example: String,
        /// Destination folder for the example (required, never overwritten)
        #[arg(short, long)]
        out: PathBuf,
    },
}

/// Every command name (and alias, and `examples` sub-command) after which a
/// bare `help` means "show help for that command".
const COMMAND_NAMES: &[&str] = &[
    "espf",
    "ref",
    "new",
    "init",
    "check",
    "validate",
    "build",
    "gen",
    "examples",
    "categories",
    "list",
    "get",
    "doctor",
    "version",
];

/// Let `help` stand in for `--help` right after a command name, so
/// `espforge new help` works like `espforge new --help`.
///
/// Only a `help` that directly follows a command name is rewritten:
/// `espforge help new` is clap's own form, `espforge new myproj help` and
/// `espforge -C help` keep `help` as a plain argument. The one name you can
/// no longer give is a project directory called `help` straight after `new`.
fn expand_help(mut args: Vec<OsString>) -> Vec<OsString> {
    // Inside `espf`, a trailing `help` is the man-page view word, rewritten
    // by `catalog::strip_view` — never into `--help` (plan 7.1). The rule:
    // the first argument after the program (skipping `-C <dir>` values) being
    // `ref` or `espf` turns the rewrite off for the whole line.
    let first_arg_is_espf = args.iter().skip(1).any(|a| {
        let s = a.to_str();
        s == Some("espf") || s == Some("ref")
    });
    if first_arg_is_espf {
        return args;
    }
    for i in 2..args.len() {
        let after_command = args[i - 1]
            .to_str()
            .is_some_and(|prev| COMMAND_NAMES.contains(&prev));
        if args[i] == "help" && after_command {
            args[i] = OsString::from("--help");
            break;
        }
    }
    args
}

fn finish(result: anyhow::Result<()>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("❌ {e:#}");
            ExitCode::from(1)
        }
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse_from(expand_help(std::env::args_os().collect()));

    if let Some(dir) = &cli.directory {
        if let Err(e) = std::env::set_current_dir(dir) {
            eprintln!("❌ cannot change to '{}': {e}", dir.display());
            return ExitCode::from(2);
        }
    }

    match cli.command {
        Some(Commands::New(args)) => commands::new::run(args),
        Some(Commands::Check(args)) => commands::check::run(args),
        Some(Commands::Build(cmd)) => {
            let mut args = cmd.args;
            args.release = cmd.release;
            commands::build::run(args)
        }
        Some(Commands::Gen(mut args)) => {
            args.stage = Stage::Generate;
            commands::build::run(args)
        }
        Some(Commands::Examples { command }) => match command {
            Some(ExamplesSub::Categories) => finish(commands::examples::run_categories()),
            Some(ExamplesSub::List { category }) => {
                finish(commands::examples::run_list(category.as_deref()))
            }
            Some(ExamplesSub::Get { example, out }) => {
                finish(commands::examples::run_get(&example, &out))
            }
            // No plumbing subcommand: the interactive browser.
            None => {
                commands::examples::run();
                ExitCode::SUCCESS
            }
        },
        Some(Commands::Espf(args)) => commands::espf::run(args),
        Some(Commands::Doctor) => {
            commands::doctor::run();
            ExitCode::SUCCESS
        }
        Some(Commands::Version) => {
            commands::version::run();
            ExitCode::SUCCESS
        }
        // No subcommand given: show help instead of erroring.
        None => {
            Cli::command()
                .print_help()
                .expect("rendering help should not fail");
            ExitCode::SUCCESS
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expand(args: &[&str]) -> Vec<String> {
        expand_help(args.iter().map(OsString::from).collect())
            .into_iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn help_after_a_command_becomes_dash_dash_help() {
        assert_eq!(
            expand(&["espforge", "new", "help"]),
            ["espforge", "new", "--help"]
        );
        assert_eq!(
            expand(&["espforge", "init", "help"]),
            ["espforge", "init", "--help"]
        );
        assert_eq!(
            expand(&["espforge", "build", "help"]),
            ["espforge", "build", "--help"]
        );
        assert_eq!(
            expand(&["espforge", "examples", "list", "help"]),
            ["espforge", "examples", "list", "--help"]
        );
        assert_eq!(
            expand(&["espforge", "-C", "dir", "new", "help", "--yes"]),
            ["espforge", "-C", "dir", "new", "--help", "--yes"]
        );
    }

    #[test]
    fn a_bare_help_after_espf_is_kept_for_the_view_parser() {
        // Plan 7.1 + drift test 14: `help right after espf must survive
        // so `espforge espf peripherals i2c help` becomes the man page.
        assert_eq!(
            expand(&["espforge", "espf", "peripherals", "i2c", "help"]),
            ["espforge", "espf", "peripherals", "i2c", "help"]
        );
        assert_eq!(
            expand(&["espforge", "ref", "i2c", "examples"]),
            ["espforge", "ref", "i2c", "examples"]
        );
        // `help` is a view word anywhere in an espf path, so even a section
        // named `help`-adjacent position is untouched.
        assert_eq!(
            expand(&["espforge", "-C", "dir", "ref", "help"]),
            ["espforge", "-C", "dir", "ref", "help"]
        );
    }

    #[test]
    fn other_uses_of_the_word_help_are_left_alone() {
        // clap's own form
        assert_eq!(
            expand(&["espforge", "help", "new"]),
            ["espforge", "help", "new"]
        );
        assert_eq!(expand(&["espforge", "help"]), ["espforge", "help"]);
        // `help` as an ordinary argument
        assert_eq!(
            expand(&["espforge", "new", "myproj", "help"]),
            ["espforge", "new", "myproj", "help"]
        );
        assert_eq!(
            expand(&["espforge", "-C", "help"]),
            ["espforge", "-C", "help"]
        );
    }

    #[test]
    fn every_command_name_is_a_real_command() {
        let cmd = Cli::command();
        for name in [
            "new", "init", "check", "validate", "build", "gen", "examples", "espf", "ref",
            "doctor", "version",
        ] {
            assert!(COMMAND_NAMES.contains(&name), "{name}");
            assert!(cmd.find_subcommand(name).is_some(), "{name}");
        }
    }
}
