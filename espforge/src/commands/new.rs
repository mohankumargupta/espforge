//! `espforge new [DIR]` (alias `init`): scaffold a new ESPF project.
//!
//! Porcelain over `build` (see cli.md): it asks for whatever is missing,
//! writes `<DIR>/<name>.espf` (canonical, via the espf writer) and a
//! `src/app.rs` stub, then runs the equivalent of
//! `espforge build <DIR>/<name>.espf --out <DIR>/build --stage generate`
//! and prints that command. With `--yes` nothing is ever prompted.

use clap::Args;
use cliclack::{Input, Select, confirm, intro, log, outro, outro_cancel};
use espforge_model::{Console, Meta, Runtime};
use espforge_options::{Answers, DerivedInputs, Kind, OptValue, chips, embedded_schema};
use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use super::answers::{AnswerArgs, ConsoleArg, RuntimeArg};
use super::build::{self, BuildArgs, Failure, Stage};
use super::spec;

/// Options that are computed from the project, never asked for.
const DERIVED: [&str; 2] = ["embassy", "wifi"];

#[derive(Args, Debug, Clone, Default)]
pub struct NewArgs {
    /// Directory for the new project (asked for when missing)
    pub dir: Option<PathBuf>,
    /// Never prompt: use defaults for everything not given as a flag
    #[arg(short = 'y', long)]
    pub yes: bool,
    #[command(flatten)]
    pub answers: AnswerArgs,
}

// ------------------------------------------------------------------------ UI

/// The prompts `new` needs, so the flow can be tested without a terminal.
trait Ui {
    fn text(&mut self, prompt: &str, default: &str) -> io::Result<String>;
    fn select(&mut self, prompt: &str, items: &[String], initial: usize) -> io::Result<usize>;
    fn confirm(&mut self, prompt: &str, initial: bool) -> io::Result<bool>;
    fn note(&mut self, msg: &str);
}

struct CliUi;

impl Ui for CliUi {
    fn text(&mut self, prompt: &str, default: &str) -> io::Result<String> {
        Input::new(prompt).default_input(default).interact()
    }

    fn select(&mut self, prompt: &str, items: &[String], initial: usize) -> io::Result<usize> {
        let entries: Vec<(usize, String, &str)> = items
            .iter()
            .cloned()
            .enumerate()
            .map(|(i, s)| (i, s, ""))
            .collect();
        Select::<usize>::new(prompt)
            .items(&entries)
            .initial_value(initial)
            .interact()
    }

    fn confirm(&mut self, prompt: &str, initial: bool) -> io::Result<bool> {
        confirm(prompt).initial_value(initial).interact()
    }

    fn note(&mut self, msg: &str) {
        let _ = log::remark(msg);
    }
}

// ------------------------------------------------------------------- errors

enum Stop {
    /// A problem with the user's input; the message is printed by `run`.
    User(String),
    Io(io::Error),
    /// `build` failed; it already printed why.
    Failed(Failure),
}

impl From<io::Error> for Stop {
    fn from(e: io::Error) -> Self {
        Stop::Io(e)
    }
}

impl From<Failure> for Stop {
    fn from(f: Failure) -> Self {
        Stop::Failed(f)
    }
}

fn user<T>(msg: impl Into<String>) -> Result<T, Stop> {
    Err(Stop::User(msg.into()))
}

// ------------------------------------------------------------------ gathering

struct Gathered {
    dir: PathBuf,
    meta: Meta,
    answers: Answers,
}

/// Collect the answers: flags first, then prompts for what is missing (or the
/// documented defaults with `--yes`).
fn gather(args: &NewArgs, ui: &mut dyn Ui) -> Result<Gathered, Stop> {
    let schema = embedded_schema();
    let a = &args.answers;
    let interactive = !args.yes;

    // Directory.
    let dir = match &args.dir {
        Some(d) => d.clone(),
        None if !interactive => return user("give a DIR when using --yes"),
        None => PathBuf::from(ui.text("Where should the project go?", "my_project")?),
    };
    if dir.as_os_str().is_empty() {
        return user("the project directory cannot be empty");
    }
    let dir_name = dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "espforge_project".to_string());

    // Name.
    let name = match &a.name {
        Some(n) => n.clone(),
        None if interactive => ui.text("Project name", &dir_name)?,
        None => dir_name.clone(),
    };
    if name.trim().is_empty() {
        return user("the project name cannot be empty");
    }

    // Chip.
    let chip = match &a.chip {
        Some(c) => {
            if !chips::is_supported_chip(c) {
                return user(format!(
                    "unknown chip \"{c}\" for --chip (expected one of: {})",
                    chips::offered_chips().join(", ")
                ));
            }
            c.clone()
        }
        None if interactive => {
            let items: Vec<String> = chips::offered_chips()
                .iter()
                .map(|c| c.to_string())
                .collect();
            let initial = items.iter().position(|c| c == "esp32c3").unwrap_or(0);
            items[ui.select("Chip", &items, initial)?].clone()
        }
        None => "esp32c3".to_string(),
    };

    // Runtime.
    let runtime = match a.runtime {
        Some(RuntimeArg::Blocking) => Runtime::Blocking,
        Some(RuntimeArg::Embassy) => Runtime::Embassy,
        None if interactive => {
            let items = vec!["blocking".to_string(), "embassy".to_string()];
            match ui.select("Runtime", &items, 0)? {
                0 => Runtime::Blocking,
                _ => Runtime::Embassy,
            }
        }
        None => Runtime::Blocking,
    };

    // Console (jtag-serial is only offered where the chip has it).
    let console = match a.console {
        Some(ConsoleArg::Uart) => Console::Uart,
        Some(ConsoleArg::JtagSerial) => Console::JtagSerial,
        None if interactive => {
            let mut items = vec!["uart".to_string()];
            if chips::has_usb_serial_jtag(&chip) {
                items.push("jtag-serial".to_string());
            }
            match ui.select("Console", &items, 0)? {
                0 => Console::Uart,
                _ => Console::JtagSerial,
            }
        }
        None => Console::Uart,
    };

    // Options given as flags.
    let mut answers = Answers::empty(0..0);
    a.apply_options(schema, &mut answers, &mut Vec::new())
        .map_err(Stop::User)?;

    // Remaining options: accept defaults, or walk options.kdl.
    if interactive {
        let eligible: Vec<_> = schema
            .options
            .iter()
            .filter(|d| {
                !DERIVED.contains(&d.name.as_str())
                    && d.compat.allows(&chip)
                    && !answers.values.contains_key(&d.name)
            })
            .collect();
        if !eligible.is_empty() && a.defaults.is_none() {
            let accept = ui.confirm(
                &format!(
                    "Accept defaults for the remaining {} options?",
                    eligible.len()
                ),
                true,
            )?;
            if !accept {
                for def in eligible {
                    let label = def.display_name.clone().unwrap_or_else(|| def.name.clone());
                    if let Some(help) = &def.help {
                        ui.note(help);
                    }
                    match &def.kind {
                        Kind::Bool => {
                            let initial = matches!(def.default, Some(OptValue::Bool(true)));
                            let v = ui.confirm(&label, initial)?;
                            if v != initial {
                                answers
                                    .apply_flag(schema, &def.name, if v { "true" } else { "false" })
                                    .map_err(Stop::User)?;
                            }
                        }
                        Kind::String => {
                            let default = match &def.default {
                                Some(OptValue::Str(s)) => s.clone(),
                                _ => String::new(),
                            };
                            let v = ui.text(&label, &default)?;
                            if v != default {
                                answers
                                    .apply_flag(schema, &def.name, &v)
                                    .map_err(Stop::User)?;
                            }
                        }
                        Kind::Choice(choices) => {
                            let initial = match &def.default {
                                Some(OptValue::Str(s)) => {
                                    choices.iter().position(|c| c == s).unwrap_or(0)
                                }
                                _ => 0,
                            };
                            let picked = ui.select(&label, choices, initial)?;
                            if picked != initial {
                                answers
                                    .apply_flag(schema, &def.name, &choices[picked])
                                    .map_err(Stop::User)?;
                            }
                        }
                        // No list options exist yet; nothing sensible to prompt.
                        Kind::List => {}
                    }
                }
            }
        }
    }

    Ok(Gathered {
        dir,
        meta: Meta {
            name,
            description: None,
            chip,
            runtime,
            console,
        },
        answers,
    })
}

// ------------------------------------------------------------------ writing

/// The `src/app.rs` stub for the chosen runtime.
fn app_stub(name: &str, runtime: Runtime) -> String {
    match runtime {
        Runtime::Blocking => format!(
            "use crate::Context;\n\
\n\
pub fn setup(ctx: &mut Context) {{\n\
\x20   let logger = ctx.logger;\n\
\x20   logger.info(\"Hello from {name}!\");\n\
}}\n\
\n\
pub fn forever(ctx: &mut Context) {{\n\
\x20   ctx.delay.delay_ms(1000);\n\
}}\n"
        ),
        Runtime::Embassy => format!(
            "use crate::Context;\n\
use embassy_executor::Spawner;\n\
\n\
pub async fn setup(ctx: &'static Context, _spawner: Spawner) {{\n\
\x20   let logger = ctx.logger;\n\
\x20   logger.info(\"Hello from {name}!\");\n\
}}\n\
\n\
pub async fn forever(ctx: &'static Context) {{\n\
\x20   ctx.delay.delay_ms(1000).await;\n\
}}\n"
        ),
    }
}

/// `new` may write into an existing directory, but never over a file.
fn check_target(dir: &Path, files: &[PathBuf]) -> Result<(), String> {
    if dir.exists() && !dir.is_dir() {
        return Err(format!("'{}' exists and is not a directory", dir.display()));
    }
    for f in files {
        if f.exists() {
            return Err(format!(
                "refusing to overwrite '{}' (already exists); pick another directory or remove it",
                f.display()
            ));
        }
    }
    Ok(())
}

struct Created {
    espf: PathBuf,
    out: PathBuf,
}

fn create(args: &NewArgs, ui: &mut dyn Ui) -> Result<Created, Stop> {
    let g = gather(args, ui)?;

    // Resolve first: nothing is written for a combination that cannot build.
    if let Err(errors) = espforge_options::resolve(&g.meta, &g.answers, &DerivedInputs::default()) {
        let msg: Vec<String> = errors
            .iter()
            .map(|e| format!("{}: {}", e.title, e.label))
            .collect();
        return user(msg.join("\n❌ "));
    }

    let slug = g.meta.crate_name();
    let espf = g.dir.join(format!("{slug}.espf"));
    let app = g.dir.join("src").join("app.rs");
    check_target(&g.dir, &[espf.clone(), app.clone()]).map_err(Stop::User)?;

    let spec_text = spec::render(&spec::blank_document(&g.meta, &g.answers))
        .map_err(|e| Stop::User(e.to_string()))?;
    std::fs::create_dir_all(g.dir.join("src"))?;
    std::fs::write(&espf, spec_text)?;
    std::fs::write(&app, app_stub(&g.meta.name, g.meta.runtime))?;

    // The equivalent build, generate stage only; compiling is the user's call.
    let out = g.dir.join("build");
    build::execute(&BuildArgs {
        file: Some(espf.clone()),
        out: out.clone(),
        stage: Stage::Generate,
        ..Default::default()
    })?;
    Ok(Created { espf, out })
}

pub fn run(args: NewArgs) -> ExitCode {
    if !args.yes && !io::stdin().is_terminal() {
        eprintln!("❌ not a terminal: pass --yes (and flags) to run `new` without prompts");
        return ExitCode::from(1);
    }
    if !args.yes {
        let _ = intro("espforge new");
    }
    match create(&args, &mut CliUi) {
        Ok(c) => {
            let msg = format!("created {}", c.espf.display());
            if args.yes {
                println!("new: {msg}");
            } else {
                let _ = outro(msg);
            }
            println!(
                "  equivalent: espforge build {} --out {}",
                c.espf.display(),
                c.out.display()
            );
            println!(
                "  next: edit src/app.rs, then cd {} && cargo build",
                c.out.display()
            );
            ExitCode::SUCCESS
        }
        Err(Stop::User(msg)) => {
            eprintln!("❌ {msg}");
            ExitCode::from(1)
        }
        Err(Stop::Io(e)) if e.kind() == io::ErrorKind::Interrupted => {
            let _ = outro_cancel("Cancelled.");
            ExitCode::from(130)
        }
        Err(Stop::Io(e)) => {
            eprintln!("❌ {e}");
            ExitCode::from(2)
        }
        Err(Stop::Failed(Failure::User)) => ExitCode::from(1),
        Err(Stop::Failed(Failure::Internal(e))) => {
            eprintln!("❌ {e:#}");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Answers prompts by substring of the prompt text; anything not scripted
    /// gets its default / initial value. Records every prompt it was asked.
    #[derive(Default)]
    struct Scripted {
        texts: Vec<(&'static str, &'static str)>,
        picks: Vec<(&'static str, &'static str)>,
        confirms: Vec<(&'static str, bool)>,
        asked: Vec<String>,
        items: HashMap<String, Vec<String>>,
    }

    impl Ui for Scripted {
        fn text(&mut self, prompt: &str, default: &str) -> io::Result<String> {
            self.asked.push(prompt.to_string());
            Ok(self
                .texts
                .iter()
                .find(|(p, _)| prompt.contains(p))
                .map(|(_, v)| v.to_string())
                .unwrap_or_else(|| default.to_string()))
        }
        fn select(&mut self, prompt: &str, items: &[String], initial: usize) -> io::Result<usize> {
            self.asked.push(prompt.to_string());
            self.items.insert(prompt.to_string(), items.to_vec());
            Ok(self
                .picks
                .iter()
                .find(|(p, _)| prompt.contains(p))
                .and_then(|(_, label)| items.iter().position(|i| i == label))
                .unwrap_or(initial))
        }
        fn confirm(&mut self, prompt: &str, initial: bool) -> io::Result<bool> {
            self.asked.push(prompt.to_string());
            Ok(self
                .confirms
                .iter()
                .find(|(p, _)| prompt.contains(p))
                .map(|(_, v)| *v)
                .unwrap_or(initial))
        }
        fn note(&mut self, _msg: &str) {}
    }

    fn yes_args(dir: PathBuf) -> NewArgs {
        NewArgs {
            dir: Some(dir),
            yes: true,
            answers: AnswerArgs {
                chip: Some("esp32c3".into()),
                runtime: Some(RuntimeArg::Blocking),
                ..Default::default()
            },
        }
    }

    fn is_user<T>(r: Result<T, Stop>) -> bool {
        matches!(r, Err(Stop::User(_)))
    }

    #[test]
    fn yes_with_flags_creates_a_working_project_without_prompts() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("my_proj");
        let mut ui = Scripted::default();
        let c = create(&yes_args(dir.clone()), &mut ui)
            .ok()
            .expect("create");
        assert!(ui.asked.is_empty(), "--yes must not prompt: {:?}", ui.asked);
        assert_eq!(c.espf, dir.join("my_proj.espf"));
        assert!(dir.join("src/app.rs").is_file());
        assert!(dir.join("build/Cargo.toml").is_file());
        assert!(dir.join("build/src/app.rs").is_file());
        let spec = std::fs::read_to_string(&c.espf).unwrap();
        assert!(
            spec.contains("<project name=\"my_proj\" />")
                && spec.contains("<options defaults=\"true\" />"),
            "{spec}"
        );
        assert!(super::super::frontend::load(&spec).is_ok());
    }

    #[test]
    fn yes_defaults_missing_answers() {
        let tmp = tempfile::tempdir().unwrap();
        let args = NewArgs {
            dir: Some(tmp.path().join("p")),
            yes: true,
            ..Default::default()
        };
        let c = create(&args, &mut Scripted::default())
            .ok()
            .expect("create");
        let spec = std::fs::read_to_string(c.espf).unwrap();
        assert!(
            spec.contains("esp32c3")
                && spec.contains("blocking")
                && spec.contains("<console type=\"uart\" />"),
            "{spec}"
        );
    }

    #[test]
    fn yes_requires_a_directory() {
        let args = NewArgs {
            yes: true,
            ..Default::default()
        };
        assert!(is_user(create(&args, &mut Scripted::default())));
    }

    #[test]
    fn bad_flags_are_user_errors_and_write_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("p");
        let mut args = yes_args(dir.clone());
        args.answers.chip = Some("esp99".into());
        assert!(is_user(create(&args, &mut Scripted::default())));
        // wifi is derived: not settable
        let mut args = yes_args(dir.clone());
        args.answers.options = vec!["wifi=true".into()];
        assert!(is_user(create(&args, &mut Scripted::default())));
        assert!(
            !dir.exists(),
            "nothing may be written for an invalid combination"
        );
    }

    #[test]
    fn existing_files_are_never_overwritten_but_other_files_are_fine() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("p");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("notes.txt"), "mine").unwrap();
        create(&yes_args(dir.clone()), &mut Scripted::default())
            .ok()
            .expect("unrelated files are fine");
        assert_eq!(
            std::fs::read_to_string(dir.join("notes.txt")).unwrap(),
            "mine"
        );
        // a second run would overwrite the espf: refused
        assert!(is_user(create(
            &yes_args(dir.clone()),
            &mut Scripted::default()
        )));
    }

    #[test]
    fn interactive_flow_asks_for_everything_missing_and_walks_options() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("proj");
        let args = NewArgs {
            dir: Some(dir.clone()),
            ..Default::default()
        };
        let mut ui = Scripted {
            texts: vec![("Project name", "demo")],
            picks: vec![
                ("Chip", "esp32c6"),
                ("Runtime", "embassy"),
                ("Console", "jtag-serial"),
            ],
            confirms: vec![("Accept defaults", false), ("Wokwi", false)],
            ..Default::default()
        };
        let c = create(&args, &mut ui).ok().expect("create");
        for p in [
            "Project name",
            "Chip",
            "Runtime",
            "Console",
            "Accept defaults",
        ] {
            assert!(
                ui.asked.iter().any(|a| a.contains(p)),
                "never asked {p}: {:?}",
                ui.asked
            );
        }
        let spec = std::fs::read_to_string(&c.espf).unwrap();
        assert_eq!(c.espf, dir.join("demo.espf"));
        assert!(
            spec.contains("<chip type=\"esp32c6\" />")
                && spec.contains("<runtime type=\"embassy\" />")
                && spec.contains("jtag-serial"),
            "{spec}"
        );
        // only the changed option is recorded
        assert!(
            spec.contains("<options defaults=\"true\" wokwi=\"false\" />"),
            "{spec}"
        );
        assert!(!dir.join("build/diagram.json").exists());
        // embassy stub
        assert!(
            std::fs::read_to_string(dir.join("src/app.rs"))
                .unwrap()
                .contains("async fn setup")
        );
    }

    #[test]
    fn flags_given_are_not_prompted_and_console_choices_follow_the_chip() {
        let tmp = tempfile::tempdir().unwrap();
        let args = NewArgs {
            dir: Some(tmp.path().join("p")),
            answers: AnswerArgs {
                name: Some("named".into()),
                chip: Some("esp32".into()),
                runtime: Some(RuntimeArg::Blocking),
                defaults: Some(true),
                ..Default::default()
            },
            ..Default::default()
        };
        let mut ui = Scripted::default();
        create(&args, &mut ui).ok().expect("create");
        for p in ["Chip", "Runtime", "Project name", "Accept defaults"] {
            assert!(
                !ui.asked.iter().any(|a| a.contains(p)),
                "{p} should not be asked"
            );
        }
        // esp32 has no USB-Serial-JTAG
        assert_eq!(ui.items["Console"], ["uart"]);
    }

    #[test]
    fn derived_options_are_never_offered() {
        let tmp = tempfile::tempdir().unwrap();
        let args = NewArgs {
            dir: Some(tmp.path().join("p")),
            ..Default::default()
        };
        let mut ui = Scripted {
            confirms: vec![("Accept defaults", false)],
            ..Default::default()
        };
        create(&args, &mut ui).ok().expect("create");
        assert!(
            !ui.asked
                .iter()
                .any(|a| a.contains("Wi-Fi") || a.contains("embassy framework")),
            "{:?}",
            ui.asked
        );
    }

    #[test]
    fn app_stubs_match_the_runtime_signatures() {
        let b = app_stub("x", Runtime::Blocking);
        assert!(b.contains("pub fn setup(ctx: &mut Context)") && b.contains("Hello from x!"));
        let e = app_stub("x", Runtime::Embassy);
        assert!(e.contains("pub async fn forever(ctx: &'static Context)"));
    }
}
