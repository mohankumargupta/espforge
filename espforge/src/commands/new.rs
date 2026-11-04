//! `espforge new`: an embedded example as an editable project.
//!
//! Two ways in: all three flags (`--example`, `--chip`, `--dir`) run straight
//! through without a terminal, and no flags asks one question at a time —
//! category, example, chip, directory — after a short sentence saying what is
//! coming. Both resolve to one [`Plan`] that a single `write` materializes, so
//! the entries cannot drift. A partial set of flags never reaches this module:
//! clap rejects it and prints the usage.
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

/// The chip a scaffold gets when the example names none.
const DEFAULT_CHIP: &str = "esp32c3";

/// Set this to keep `espforge new` from ever blocking on a prompt.
const NO_PROMPT: &str = "ESPFORGE_NO_PROMPT";

/// What `espforge new` says before its first question: the point of the
/// subcommand is to explain the shape of what is coming.
const START: &str = "As a starting point for your espforge project, start with an example, and build from there.\n\nFirst pick an example category, then an example.";

/// The answers that skip the wizard. All three or none: clap's `requires_all`
/// refuses a partial set with the command's usage, so a script cannot silently
/// fall back to asking.
#[derive(Args, Debug, Clone, Default)]
pub struct NewArgs {
    /// Example to start from (category/name, or a short name like blink)
    #[arg(long, value_name = "EXAMPLE", requires_all = ["chip", "dir"])]
    pub example: Option<String>,
    /// Target chip, e.g. esp32c3
    #[arg(long, value_name = "CHIP", requires_all = ["example", "dir"])]
    pub chip: Option<String>,
    /// Project directory (must be absent or empty)
    #[arg(short, long, value_name = "DIR", requires_all = ["example", "chip"])]
    pub dir: Option<PathBuf>,
}

impl NewArgs {
    /// Whether every answer came from the command line, so nothing is asked.
    fn flagged(&self) -> bool {
        self.example.is_some() && self.chip.is_some() && self.dir.is_some()
    }
}

pub fn run(args: NewArgs) -> ExitCode {
    exit_code(execute(&args))
}

/// The only entry: flags when all three were given, the terminal otherwise.
fn execute(args: &NewArgs) -> Result<(), Failure> {
    if args.flagged() {
        let plan = flagged_plan(args)?;
        let done = write(&plan)?;
        return report_new(&done, &plan, false);
    }

    if !prompts() {
        return Err(user(
            "`espforge new` asks its questions on a terminal; \
             run it from an interactive shell, or pass --example, --chip and --dir",
        ));
    }

    cliclack::intro("espforge new").map_err(|_| cancelled())?;
    cliclack::note("Getting started", START).map_err(|_| cancelled())?;

    let plan = prompted_plan()?;
    let done = write(&plan)?;
    report_new(&done, &plan, true)
}

/// Whether `new` may ask the terminal: both ends are terminals and
/// `ESPFORGE_NO_PROMPT` is not set.
fn prompts() -> bool {
    std::io::stdin().is_terminal() && std::io::stdout().is_terminal() && !opted_out()
}

/// `ESPFORGE_NO_PROMPT` set to anything but an explicit off value.
///
/// The escape hatch a terminal check alone cannot give: a script or CI runner
/// with a TTY must never sit waiting for a keystroke.
fn opted_out() -> bool {
    std::env::var_os(NO_PROMPT)
        .is_some_and(|value| !matches!(value.to_str(), Some("") | Some("0") | Some("false")))
}

/// Everything `new` decided before writing anything.
#[derive(Debug)]
struct Plan {
    example: &'static Example,
    chip: String,
    dir: PathBuf,
}

fn prompted_plan() -> Result<Plan, Failure> {
    let example = prompt_example()?;
    let chip = prompt_chip(example)?;
    let dir = prompt_dir(example)?;

    Ok(Plan { example, chip, dir })
}

/// The plan the three flags spell out. `flagged` already proved they are all
/// here, so a missing one is a bug in the caller rather than in the user.
fn flagged_plan(args: &NewArgs) -> Result<Plan, Failure> {
    let input = args
        .example
        .as_deref()
        .ok_or_else(|| Failure::Internal("`new` ran with flags but no --example".into()))?;
    let (_, example) = resolve_name(input).map_err(user)?;
    let chip = args
        .chip
        .clone()
        .ok_or_else(|| Failure::Internal("`new` ran with flags but no --chip".into()))?;
    let dir = args
        .dir
        .clone()
        .ok_or_else(|| Failure::Internal("`new` ran with flags but no --dir".into()))?;

    Ok(Plan { example, chip, dir })
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

    // The names are descriptive enough on their own; the banner title would
    // only add noise here.
    let mut prompt = cliclack::select("Example to start from").filter_mode();
    for example in &category.examples {
        prompt = prompt.item(example.name.as_str(), &example.name, "");
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
    // Just the chip names: the descriptions belong in docs, not in a picker.
    for chip in &choices {
        prompt = prompt.item(chip.clone(), chip, "");
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

/// Ask where the project goes. The field starts blank; a blank answer means a
/// directory named after the example, so pressing Enter is always a valid
/// answer.
fn prompt_dir(example: &Example) -> Result<PathBuf, Failure> {
    let placeholder = format!("leave blank for '{}'", example.name);
    let dir: String = cliclack::input("Project directory")
        .placeholder(&placeholder)
        .required(false)
        .validate(|input: &String| {
            let input = input.trim();
            if input.is_empty() {
                return Ok(());
            }
            match directory_problem(Path::new(input)) {
                Some(problem) => Err(problem),
                None => Ok(()),
            }
        })
        .interact()
        .map_err(|_| cancelled())?;
    Ok(destination(&dir, example))
}

/// The directory a typed answer names: blank is a directory named after the
/// example.
fn destination(input: &str, example: &Example) -> PathBuf {
    let input = input.trim();
    if input.is_empty() {
        PathBuf::from(&example.name)
    } else {
        PathBuf::from(input)
    }
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
    chips::check_chip(&plan.chip).map_err(user)?;

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

    if !wizard {
        if unbuilt {
            println!(
                "note: {} is not built for this example (built for: {})",
                plan.chip,
                plan.example.chips.join(", ")
            );
        }
        println!(
            "new: created '{spec_name}' ({}) in {}",
            plan.chip,
            plan.dir.display()
        );
        println!("  next: cd {} && espforge build", plan.dir.display());
        return Ok(());
    }

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
    use espforge_examples::{find, resolve_name};

    use super::*;

    /// A plan with the answers a test would have typed: no prompting involved.
    fn planned(example: &str, chip: Option<&str>, dir: &Path) -> Result<Plan, Failure> {
        let (_, example) = resolve_name(example).map_err(user)?;
        Ok(Plan {
            example,
            chip: chip.map_or_else(|| default_chip(example), str::to_string),
            dir: dir.to_path_buf(),
        })
    }

    fn scaffold(example: &str, chip: Option<&str>, dir: &Path) -> Result<(), Failure> {
        write(&planned(example, chip, dir)?).map(|_| ())
    }

    /// The flags a non-interactive run gets: all three, always.
    fn flags(example: &str, chip: &str, dir: &Path) -> NewArgs {
        NewArgs {
            example: Some(example.into()),
            chip: Some(chip.into()),
            dir: Some(dir.to_path_buf()),
        }
    }

    #[test]
    fn the_three_flags_build_the_plan_without_asking() {
        let plan = flagged_plan(&flags("blink", "esp32s3", Path::new("my-app"))).unwrap();
        assert_eq!(plan.example.name, "blink");
        assert_eq!(plan.chip, "esp32s3");
        assert_eq!(plan.dir, PathBuf::from("my-app"));
    }

    #[test]
    fn a_flagged_run_scaffolds_without_a_terminal() {
        let scratch = tempfile::tempdir().unwrap();
        let dir = scratch.path().join("flagged");
        execute(&flags("01.Basics/helloworld", "esp32c6", &dir)).unwrap();

        assert!(dir.join("helloworld.espf").is_file());
        let spec = std::fs::read_to_string(dir.join("helloworld.espf")).unwrap();
        assert!(spec.contains("<chip type=\"esp32c6\" />"), "{spec}");
    }

    #[test]
    fn a_flagged_unknown_example_is_a_user_error() {
        let scratch = tempfile::tempdir().unwrap();
        let dir = scratch.path().join("typo");
        assert!(matches!(
            execute(&flags("blinkk", "esp32c3", &dir)),
            Err(Failure::User)
        ));
        assert!(!dir.exists(), "nothing may be written");
    }

    #[test]
    fn a_flagged_unknown_chip_is_a_user_error() {
        let scratch = tempfile::tempdir().unwrap();
        let dir = scratch.path().join("bad-chip");
        assert!(matches!(
            execute(&flags("blink", "esp99", &dir)),
            Err(Failure::User)
        ));
        assert!(!dir.exists(), "nothing may be written");
    }

    #[test]
    fn scaffolds_a_project_that_generates() {
        let scratch = tempfile::tempdir().unwrap();
        let dir = scratch.path().join("my-app");
        scaffold("blink", None, &dir).unwrap();

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
        scaffold("blink", Some("esp32s3"), &dir).unwrap();

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
        scaffold("blink", Some("esp32c6"), &dir).unwrap();

        let spec = std::fs::read_to_string(dir.join("blink.espf")).unwrap();
        assert!(spec.contains("<chip type=\"esp32c6\" />"), "{spec}");
    }

    #[test]
    fn an_unknown_chip_is_a_user_error() {
        let scratch = tempfile::tempdir().unwrap();
        let dir = scratch.path().join("bad-chip");
        assert!(matches!(
            scaffold("blink", Some("esp99"), &dir),
            Err(Failure::User)
        ));
        assert!(!dir.exists(), "nothing may be written");
    }

    #[test]
    fn a_chip_espforge_cannot_build_yet_is_a_user_error() {
        let scratch = tempfile::tempdir().unwrap();
        let dir = scratch.path().join("p4-app");
        assert!(matches!(
            scaffold("blink", Some(chips::NOT_YET_SUPPORTED[0]), &dir),
            Err(Failure::User)
        ));
        assert!(!dir.exists(), "nothing may be written");
    }

    #[test]
    fn the_scaffolded_project_passes_check() {
        let scratch = tempfile::tempdir().unwrap();
        let dir = scratch.path().join("my-app");
        scaffold("blink", None, &dir).unwrap();

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
        scaffold("01.Basics/helloworld", None, &dir).unwrap();
        assert!(dir.join("helloworld.espf").is_file());
    }

    #[test]
    fn a_non_empty_directory_is_a_user_error() {
        let scratch = tempfile::tempdir().unwrap();
        let dir = scratch.path().join("taken");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("keep.txt"), "mine").unwrap();

        assert!(matches!(scaffold("blink", None, &dir), Err(Failure::User)));
        assert!(!dir.join("blink.espf").exists(), "nothing may be written");
    }

    #[test]
    fn an_unknown_example_is_a_user_error() {
        let scratch = tempfile::tempdir().unwrap();
        let dir = scratch.path().join("typo");
        assert!(matches!(scaffold("blinkk", None, &dir), Err(Failure::User)));
        assert!(!dir.exists());
    }

    #[test]
    fn an_empty_directory_answer_means_the_example_name() {
        let blink = find("01.Basics/blink").expect("blink must exist");
        assert_eq!(destination("", blink), PathBuf::from("blink"));
        assert_eq!(destination("   ", blink), PathBuf::from("blink"));
        assert_eq!(destination("my-app", blink), PathBuf::from("my-app"));
    }

    #[test]
    fn new_explains_the_questions_before_asking_them() {
        assert!(START.contains("start with an example"), "{START}");
        assert!(START.contains("category"), "{START}");
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
