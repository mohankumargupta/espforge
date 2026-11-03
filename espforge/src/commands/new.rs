//! `espforge new`: an embedded example as an editable project.
//!
//! There are two ways in — flags for scripts, and an interactive wizard when
//! the terminal allows it — and both resolve to the same [`Plan`] and go
//! through the same write, so the wizard cannot drift from the flags.
//!
//! Resolving a name and writing the tree belong to `espforge_examples`; editing
//! the spec's chip belongs to `espforge_core`; this module only asks, in the
//! order the answers are needed, and reports what was written.

use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Args;
use espforge_core::spec;
use espforge_elements::chips;
use espforge_examples::{
    Category, Example, MaterializeError, Materialized, materialize_spec, project_spec, resolve_name,
};

use super::failure::{Failure, exit_code, user};
use super::report;

/// The chip a scaffold gets when neither a flag nor the example says.
const DEFAULT_CHIP: &str = "esp32c3";

/// Set this to keep `espforge new` from ever blocking on a prompt.
const NO_PROMPT: &str = "ESPFORGE_NO_PROMPT";

#[derive(Args, Debug, Clone, Default)]
pub struct NewArgs {
    /// Example to scaffold: a short name (`blink`) or category key (`01.Basics/blink`)
    #[arg(short, long, value_name = "EXAMPLE")]
    pub example: Option<String>,
    /// Target chip, e.g. esp32c3, esp32s3 (default: the example's own chip)
    #[arg(short, long, value_name = "CHIP")]
    pub chip: Option<String>,
    /// New directory for the project (must be absent or empty)
    #[arg(short, long, value_name = "DIR")]
    pub dir: Option<PathBuf>,
}

/// Whether `new` may ask the terminal for what a flag did not supply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Ask, but only when both ends are terminals and the user has not opted out.
    Auto,
    /// Never ask: a missing flag is an error. Tests and scripts.
    Never,
}

impl Mode {
    /// The mode this terminal allows: two terminals and no opt-out, or
    /// [`Mode::Never`].
    fn for_terminal() -> Mode {
        let terminal = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
        if terminal && !opted_out() {
            Mode::Auto
        } else {
            Mode::Never
        }
    }

    fn prompts(self) -> bool {
        matches!(self, Mode::Auto)
    }
}

/// `ESPFORGE_NO_PROMPT` set to anything but an explicit off value.
///
/// The escape hatch a terminal check alone cannot give: a script or CI runner
/// with a TTY must never sit waiting for a keystroke.
fn opted_out() -> bool {
    std::env::var_os(NO_PROMPT)
        .is_some_and(|value| !matches!(value.to_str(), Some("") | Some("0") | Some("false")))
}

pub fn run(args: NewArgs) -> ExitCode {
    exit_code(execute(&args))
}

/// The standard entry point: asks the terminal when it can, never otherwise.
pub fn execute(args: &NewArgs) -> Result<(), Failure> {
    execute_with(args, Mode::for_terminal())
}

/// Everything `new` decided before writing anything.
#[derive(Debug)]
struct Plan {
    example: &'static Example,
    chip: String,
    dir: PathBuf,
}

/// `execute` with prompting decided by the caller instead of the environment.
///
/// Every test goes through `Mode::Never`: nothing here may wait for a key, and
/// whether the test runner happens to own a terminal must not change results.
pub fn execute_with(args: &NewArgs, mode: Mode) -> Result<(), Failure> {
    // The wizard is the whole flow or none of it: with all three flags there is
    // nothing to ask, so the output stays plain text.
    let complete = args.example.is_some() && args.chip.is_some() && args.dir.is_some();
    let wizard = mode.prompts() && !complete;
    if wizard {
        cliclack::intro("espforge new").map_err(|_| cancelled())?;
    }

    let plan = plan(args, wizard)?;
    let done = write(&plan)?;
    report_new(&done, &plan, wizard)?;
    Ok(())
}

fn plan(args: &NewArgs, wizard: bool) -> Result<Plan, Failure> {
    let example = match &args.example {
        Some(name) => resolve(name)?,
        None if wizard => prompt_example()?,
        None => return Err(missing("--example <EXAMPLE>")),
    };

    let chip = match &args.chip {
        Some(name) => {
            chips::check_chip(name).map_err(user)?;
            name.clone()
        }
        None if wizard => prompt_chip(example)?,
        None => default_chip(example),
    };

    let dir = match &args.dir {
        Some(dir) => dir.clone(),
        None if wizard => prompt_dir(example)?,
        None => return Err(missing("--dir <DIR>")),
    };

    Ok(Plan { example, chip, dir })
}

fn resolve(name: &str) -> Result<&'static Example, Failure> {
    resolve_name(name).map(|(_, example)| example).map_err(user)
}

fn missing(flag: &str) -> Failure {
    user(format!(
        "missing required flag `{flag}` (run `espforge new` in a terminal to be asked)"
    ))
}

/// The chip a scaffold gets when nothing was chosen: what the example is built
/// for, then what its own spec says, then [`DEFAULT_CHIP`].
///
/// `example.toml` is the only record of what an example was actually built
/// against, so it wins over the chip its spec happens to spell.
fn default_chip(example: &Example) -> String {
    example
        .chips
        .first()
        .cloned()
        .or_else(|| spec::spec_chip(&example.content).ok().map(str::to_string))
        .unwrap_or_else(|| DEFAULT_CHIP.to_string())
}

/// Choose the example in two steps: the catalog is too long to scan as one
/// list, so pick a category first and only then its examples.
fn prompt_example() -> Result<&'static Example, Failure> {
    let category = prompt_category()?;

    let mut prompt = cliclack::select("Example to start from").filter_mode();
    for example in &category.examples {
        let hint = example.title.as_deref().unwrap_or("");
        prompt = prompt.item(example.name.as_str(), &example.name, hint);
    }
    let name: &str = prompt.interact().map_err(|_| cancelled())?;
    category
        .examples
        .iter()
        .find(|example| example.name == name)
        .ok_or_else(|| Failure::Internal(format!("example '{name}' left the catalog")))
}

fn prompt_category() -> Result<&'static Category, Failure> {
    let mut prompt = cliclack::select("Category");
    for category in espforge_examples::categories() {
        let count = category.examples.len();
        let hint = format!("{count} example{}", if count == 1 { "" } else { "s" });
        prompt = prompt.item(category.name.as_str(), category_label(&category.name), hint);
    }
    let name: &str = prompt.interact().map_err(|_| cancelled())?;
    espforge_examples::categories()
        .iter()
        .find(|category| category.name == name)
        .ok_or_else(|| Failure::Internal(format!("category '{name}' left the catalog")))
}

/// A category name without its ordering prefix: `01.Basics` reads `Basics`.
///
/// The number orders the catalog, it is not part of the name a user picks.
fn category_label(name: &str) -> &str {
    let rest = name.trim_start_matches(|c: char| c.is_ascii_digit());
    let rest = rest.strip_prefix('.').unwrap_or(rest);
    if rest.is_empty() { name } else { rest }
}

fn prompt_chip(example: &Example) -> Result<String, Failure> {
    let choices = chip_choices(example);
    let default = choices
        .first()
        .cloned()
        .unwrap_or_else(|| default_chip(example));

    let mut prompt = cliclack::select("Target chip");
    if choices.contains(&default) {
        prompt = prompt.initial_value(default);
    }
    for chip in &choices {
        let about = chips::description(chip).unwrap_or("supported target");
        let hint = if example.chips.iter().any(|built| built == chip) {
            format!("{about} · built for this example")
        } else {
            format!("{about} · not built for this example")
        };
        prompt = prompt.item(chip.clone(), chip, hint);
    }
    prompt.interact().map_err(|_| cancelled())
}

/// The chips to offer, the example's own first: those are the ones its harness
/// builds, so they are the ones that come with a promise.
fn chip_choices(example: &Example) -> Vec<String> {
    let offered = chips::offered_chips();
    let mut choices: Vec<String> = example
        .chips
        .iter()
        .filter(|chip| offered.contains(&chip.as_str()))
        .cloned()
        .collect();
    for chip in offered {
        if !choices.iter().any(|known| known.as_str() == chip) {
            choices.push(chip.to_string());
        }
    }
    choices
}

fn prompt_dir(example: &Example) -> Result<PathBuf, Failure> {
    let default = format!("./{}", example.name);
    let dir: String = cliclack::input("Project directory")
        .placeholder(&default)
        .default_input(&default)
        .validate(|input: &String| match directory_problem(Path::new(input)) {
            Some(problem) => Err(problem),
            None => Ok(()),
        })
        .interact()
        .map_err(|_| cancelled())?;
    Ok(PathBuf::from(dir))
}

/// Why `dir` is not available, in the prompt's words.
///
/// The same rule `materialize_spec` enforces, asked before the write so the
/// wizard cannot accept a destination the write would refuse.
fn directory_problem(dir: &Path) -> Option<&'static str> {
    if dir.is_file() {
        Some("that path is an existing file")
    } else if std::fs::read_dir(dir).is_ok_and(|mut entries| entries.next().is_some()) {
        Some("that directory already exists and is not empty")
    } else {
        None
    }
}

/// Write the plan's tree, with the chosen chip written into the spec.
fn write(plan: &Plan) -> Result<Materialized, Failure> {
    let base = project_spec(plan.example);
    let spec = spec::spec_with_chip(&base, &plan.chip).map_err(|error| {
        Failure::Internal(format!(
            "cannot set the chip in '{}': {error}",
            plan.example.name
        ))
    })?;

    materialize_spec(plan.example, &plan.dir, &spec).map_err(|error| match error {
        MaterializeError::Conflict(message) => user(message),
        MaterializeError::Io(error) => {
            Failure::Internal(format!("cannot write '{}': {error}", plan.dir.display()))
        }
    })
}

fn report_new(done: &Materialized, plan: &Plan, wizard: bool) -> Result<(), Failure> {
    let spec_name = done.spec.file_name().unwrap_or_default().to_string_lossy();

    // A chip outside the example's own list is allowed, but it is a promise
    // espforge has not kept: say so once, in the CLI's `note:` voice.
    let unbuilt =
        !plan.example.chips.is_empty() && !plan.example.chips.iter().any(|chip| chip == &plan.chip);

    if wizard {
        if unbuilt {
            cliclack::note(
                "Not verified",
                format!(
                    "{} is not built for this example (built for: {})",
                    plan.chip,
                    plan.example.chips.join(", ")
                ),
            )
            .map_err(|_| cancelled())?;
        }
        cliclack::note(
            "Next steps",
            format!("cd {}\nespforge build", plan.dir.display()),
        )
        .map_err(|_| cancelled())?;
        cliclack::outro(format!(
            "Created '{spec_name}' ({}) in {}",
            plan.chip,
            plan.dir.display()
        ))
        .map_err(|_| cancelled())?;
    } else {
        println!(
            "new: created '{spec_name}' ({}) in {}",
            plan.chip,
            plan.dir.display()
        );
        if unbuilt {
            report::notes(&[format!(
                "note: {} is not built for this example (built for: {})",
                plan.chip,
                plan.example.chips.join(", ")
            )]);
        }
        println!();
        println!("Next steps:");
        println!("  cd {}", plan.dir.display());
        println!("  espforge build");
    }
    Ok(())
}

/// Backing out is not a failure: say nothing was written, then exit 0.
fn cancelled() -> Failure {
    let _ = cliclack::outro_cancel("nothing was written");
    Failure::Cancelled
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use espforge_core::project;
    use espforge_core::source::resolve_source;
    use espforge_examples::find;

    use super::*;

    fn args(example: &str, dir: &Path) -> NewArgs {
        NewArgs {
            example: Some(example.into()),
            dir: Some(dir.to_path_buf()),
            chip: None,
        }
    }

    /// No test may prompt: `Mode::Never` is the whole point of the split.
    fn never(args: &NewArgs) -> Result<(), Failure> {
        execute_with(args, Mode::Never)
    }

    #[test]
    fn scaffolds_a_project_that_generates() {
        let scratch = tempfile::tempdir().unwrap();
        let dir = scratch.path().join("my-app");
        never(&args("blink", &dir)).unwrap();

        assert!(dir.join("blink.espf").is_file());
        assert!(dir.join("src").join("app.rs").is_file());
        assert!(dir.join("diagram.json").is_file());
        assert!(!dir.join("example.toml").exists(), "harness file leaked");
        assert!(!dir.join("app").exists(), "example layout leaked");

        let spec = std::fs::read_to_string(dir.join("blink.espf")).unwrap();
        assert!(spec.contains("# espforge"), "{spec}");
        assert!(spec.contains("BLINK EXAMPLE"), "{spec}");
        // The default chip is the one the example is built for, written as-is.
        assert!(spec.contains("<chip type=\"esp32c3\" />"), "{spec}");

        // The scaffold and the discovery agree about where the spec and the app
        // logic live: generating the new project is the proof.
        let source = resolve_source(None, None, &dir).unwrap_or_else(|e| panic!("{e}"));
        project::generate(&source, &dir.join("build")).unwrap_or_else(|e| panic!("{e:?}"));
        assert!(dir.join("build/Cargo.toml").is_file());
        assert!(dir.join("build/src/app.rs").is_file());
    }

    #[test]
    fn the_default_chip_is_the_one_the_example_is_built_for() {
        let blink = find("01.Basics/blink").expect("blink must exist");
        assert_eq!(blink.chips.first().map(String::as_str), Some("esp32c3"));
        assert_eq!(default_chip(blink), "esp32c3");
    }

    #[test]
    fn scaffolds_with_an_explicit_chip_and_still_checks_clean() {
        let scratch = tempfile::tempdir().unwrap();
        let dir = scratch.path().join("s3-app");
        let mut args = args("blink", &dir);
        args.chip = Some("esp32s3".into());
        never(&args).unwrap();

        let spec = std::fs::read_to_string(dir.join("blink.espf")).unwrap();
        assert!(spec.contains("<chip type=\"esp32s3\" />"), "{spec}");
        assert!(
            spec.contains("BLINK EXAMPLE"),
            "the banner must survive: {spec}"
        );

        let source = resolve_source(None, None, &dir).unwrap();
        project::generate(&source, &dir.join("build")).unwrap();
        let cargo = std::fs::read_to_string(dir.join("build/Cargo.toml")).unwrap();
        assert!(cargo.contains("\"esp32s3\""), "{cargo}");

        // The chip the spec now declares is a chip the project validates for.
        let checked = crate::commands::check::execute(&crate::commands::check::CheckArgs {
            file: Some(dir.join("blink.espf")),
            ..Default::default()
        });
        assert!(checked.is_ok(), "check must accept the rewritten spec");
    }

    #[test]
    fn a_chip_the_example_is_not_built_for_still_scaffolds() {
        let scratch = tempfile::tempdir().unwrap();
        let dir = scratch.path().join("c6-app");
        let mut args = args("blink", &dir);
        args.chip = Some("esp32c6".into());
        never(&args).unwrap();

        let spec = std::fs::read_to_string(dir.join("blink.espf")).unwrap();
        assert!(spec.contains("<chip type=\"esp32c6\" />"), "{spec}");
    }

    #[test]
    fn an_unknown_chip_is_a_user_error() {
        let scratch = tempfile::tempdir().unwrap();
        let dir = scratch.path().join("bad-chip");
        let mut args = args("blink", &dir);
        args.chip = Some("esp99".into());
        assert!(matches!(never(&args), Err(Failure::User)));
        assert!(!dir.exists(), "nothing may be written");
    }

    #[test]
    fn a_chip_espforge_cannot_build_yet_is_a_user_error() {
        let scratch = tempfile::tempdir().unwrap();
        let dir = scratch.path().join("p4-app");
        let mut args = args("blink", &dir);
        args.chip = Some(chips::NOT_YET_SUPPORTED[0].into());
        assert!(matches!(never(&args), Err(Failure::User)));
        assert!(!dir.exists(), "nothing may be written");
    }

    #[test]
    fn the_scaffolded_project_passes_check() {
        let scratch = tempfile::tempdir().unwrap();
        let dir = scratch.path().join("my-app");
        never(&args("blink", &dir)).unwrap();

        let checked = crate::commands::check::execute(&crate::commands::check::CheckArgs {
            file: Some(dir.join("blink.espf")),
            ..Default::default()
        });
        assert!(checked.is_ok(), "check must accept a scaffolded project");
    }

    #[test]
    fn a_full_category_key_scaffolds_too() {
        let scratch = tempfile::tempdir().unwrap();
        let dir = scratch.path().join("hello");
        never(&args("01.Basics/helloworld", &dir)).unwrap();
        assert!(dir.join("helloworld.espf").is_file());
    }

    #[test]
    fn a_non_empty_directory_is_a_user_error() {
        let scratch = tempfile::tempdir().unwrap();
        let dir = scratch.path().join("taken");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("keep.txt"), "mine").unwrap();

        assert!(matches!(never(&args("blink", &dir)), Err(Failure::User)));
        assert!(!dir.join("blink.espf").exists(), "nothing may be written");
    }

    #[test]
    fn an_unknown_example_is_a_user_error() {
        let scratch = tempfile::tempdir().unwrap();
        let dir = scratch.path().join("typo");
        assert!(matches!(never(&args("blinkk", &dir)), Err(Failure::User)));
        assert!(!dir.exists());
    }

    #[test]
    fn missing_flags_without_a_terminal_are_user_errors() {
        assert!(matches!(never(&NewArgs::default()), Err(Failure::User)));

        // A partial plan names what it still needs instead of guessing.
        let scratch = tempfile::tempdir().unwrap();
        let partial = NewArgs {
            example: Some("blink".into()),
            ..Default::default()
        };
        assert!(matches!(never(&partial), Err(Failure::User)));
        assert!(!scratch.path().join("blink").exists());
    }

    #[test]
    fn category_labels_drop_the_ordering_prefix() {
        assert_eq!(category_label("01.Basics"), "Basics");
        assert_eq!(category_label("06.Displays"), "Displays");
        // No prefix to drop, and nothing left to show but the name itself.
        assert_eq!(category_label("Basics"), "Basics");
        assert_eq!(category_label("01"), "01");
    }

    #[test]
    fn the_catalog_offers_categories_and_their_examples() {
        let categories = espforge_examples::categories();
        assert!(!categories.is_empty());
        for category in categories {
            assert!(
                !category.examples.is_empty(),
                "{} is offered with nothing to pick",
                category.name
            );
            assert!(
                !category_label(&category.name).is_empty(),
                "{} needs a readable label",
                category.name
            );
        }
    }

    #[test]
    fn the_chip_list_starts_with_what_the_example_is_built_for() {
        let blink = find("01.Basics/blink").expect("blink must exist");
        let choices = chip_choices(blink);
        assert_eq!(&choices[..2], ["esp32c3", "esp32s3"]);
        assert!(
            !choices.contains(&"esp32p4".to_string()),
            "a chip espforge cannot build is not offered: {choices:?}"
        );
    }
}
