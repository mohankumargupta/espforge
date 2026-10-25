//! `espforge examples`: turn an embedded example into an editable folder.
//!
//! Bare `examples` is porcelain (cli.md layer 2): a top-level category picker
//! followed by a picker of the examples inside the chosen category (categories
//! without examples are never offered), then an `Input` prompt for the
//! destination folder pre-filled with the example's own directory name — Enter
//! accepts it, the user may retype it. The chosen example is materialized to
//! that folder verbatim. Then the one question that matters by default: which
//! chip (Enter keeps the example's; a different one is rewritten into the
//! copied `.espf`). Finally "Generate the project now?" (default No) runs
//! `build --stage generate` on the folder. It never builds silently, and it
//! always prints the `next:` line.
//!
//! The subcommands `categories` / `list` / `get` are the plumbing surface from
//! `cli.md`: non-interactive, plain output, they only delegate to
//! `espforge_examples`. Porcelain never duplicates plumbing logic: picking,
//! defaults and printed next-steps live here; copying lives in
//! `espforge_examples::materialize`.
use std::io;
use std::path::Path;

use anyhow::Context;

use cliclack::{Input, Select, confirm, intro, log, outro, outro_cancel};
use espforge_core::spec::{chip_value, set_chip};
use espforge_examples::{Category, Example};

use super::build::{self, BuildArgs, Failure, Stage};

/// Name of the ESPF source file inside each example directory.
const EXAMPLE_FILE: &str = "example.espf.txt";

pub fn run() {
    match run_ui() {
        Ok(()) => {}
        // Esc / Ctrl-C inside a cliclack prompt surfaces as `Interrupted`.
        Err(err) if err.kind() == io::ErrorKind::Interrupted => {
            let _ = outro_cancel("Cancelled.");
            std::process::exit(130);
        }
        Err(err) => {
            eprintln!("⚠️ Could not run the example browser: {err}");
            std::process::exit(2);
        }
    }
}

fn run_ui() -> io::Result<()> {
    intro("espforge examples")?;

    let categories = espforge_examples::categories();
    if categories.is_empty() {
        let _ = outro_cancel("No examples are embedded in this build.");
        return Ok(());
    }

    // Top-level picker: the example categories that actually have examples.
    let entries: Vec<(usize, String, String)> = categories
        .iter()
        .enumerate()
        .map(|(i, category)| (i, category.name.clone(), category_hint(category)))
        .collect();
    let category_index = Select::<usize>::new("Select a category")
        .items(&entries)
        .interact()?;
    let category = &categories[category_index];

    // Second picker: the examples inside the chosen category.
    let entries: Vec<(usize, String, String)> = category
        .examples
        .iter()
        .enumerate()
        .map(|(i, example)| (i, example.name.clone(), example_hint(example)))
        .collect();
    let example_index = Select::<usize>::new(format!("Select an example in {}", category.name))
        .items(&entries)
        .interact()?;
    let example = &category.examples[example_index];

    // Destination prompt: pre-filled with the example's own directory name;
    // Enter accepts it, the user may retype it. Empty input falls back to the
    // default, so the suggested destination cannot be lost to a stray Enter.
    let dest: String = Input::new(format!(
        "Where should '{}/{}' be copied to?",
        category.name, example.name
    ))
    .default_input(&example.name)
    .interact()?;

    // Porcelain delegates the copying to the plumbing layer. A conflict (the
    // destination already holds files) cancels instead of overwriting.
    let written = match espforge_examples::materialize(example, Path::new(&dest)) {
        Ok(written) => written,
        Err(err) => {
            let _ = outro_cancel(format!("Could not copy the example: {err}"));
            std::process::exit(2);
        }
    };
    log::success(format!(
        "Copied {}/{} to {} ({} files)",
        category.name,
        example.name,
        dest,
        written.len()
    ))?;

    let espf_path = Path::new(&dest).join(EXAMPLE_FILE);

    // The only question asked by default: which chip.
    choose_chip(&espf_path)?;

    // Never builds silently: the default answer is No.
    let generate = confirm("Generate the project now?")
        .initial_value(false)
        .interact()?;
    let mut generated = false;
    if generate {
        let out = Path::new(&dest).join("build");
        match build::execute(&BuildArgs {
            file: Some(espf_path.clone()),
            out,
            stage: Stage::Generate,
            ..Default::default()
        }) {
            Ok(()) => generated = true,
            // The reason (diagnostics) was already printed by `build`.
            Err(Failure::User) => {
                let _ = log::warning("The project was not generated; fix the problems above.");
            }
            Err(Failure::Internal(e)) => {
                let _ = log::error(format!("{e:#}"));
            }
        }
    }

    outro(format!("Example ready in {dest}"))?;
    if generated {
        println!("  next: cd {dest}/build && cargo build");
    } else {
        println!("  next: cd {dest} → edit the example → espforge build {dest}/{EXAMPLE_FILE}");
    }

    Ok(())
}

/// Ask for the chip (Enter keeps the example's) and rewrite the copied `.espf`
/// when it changes.
fn choose_chip(espf_path: &Path) -> io::Result<()> {
    let text = std::fs::read_to_string(espf_path)?;
    let Some((_, current)) = chip_value(&text) else {
        return Ok(());
    };

    let chips = espforge_options::chips::offered_chips();
    let entries: Vec<(usize, String, String)> = chips
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let hint = if *c == current {
                "this example's chip"
            } else {
                ""
            };
            (i, c.to_string(), hint.to_string())
        })
        .collect();
    let initial = chips.iter().position(|c| *c == current).unwrap_or(0);
    let picked = Select::<usize>::new("Chip")
        .items(&entries)
        .initial_value(initial)
        .interact()?;
    let chip = chips[picked];

    if chip != current {
        if let Some(new_text) = set_chip(&text, chip) {
            std::fs::write(espf_path, new_text)?;
            log::warning(format!(
                "Changed the chip to {chip}. Pin numbers in this example were written for {current}; check them for {chip}."
            ))?;
        }
    }
    Ok(())
}

/// `examples categories`: plain `category<TAB>N examples` lines.
pub fn run_categories() -> anyhow::Result<()> {
    let categories = espforge_examples::categories();
    if categories.is_empty() {
        anyhow::bail!("no examples are embedded in this build");
    }
    for category in categories {
        println!("{}\t{}", category.name, category_hint(category));
    }
    Ok(())
}

/// `examples list [category]`: plain `category/name<TAB>title` lines, for
/// every example of every category when no category is given.
pub fn run_list(category: Option<&str>) -> anyhow::Result<()> {
    let lines = espforge_examples::list(category)
        .map_err(|err| anyhow::anyhow!("{err}"))
        .context("failed to list examples")?;
    for line in lines {
        println!("{line}");
    }
    Ok(())
}

/// `examples get <category>/<name> --out <dir>`: materialize the embedded
/// example tree at the user-chosen destination.
pub fn run_get(example_path: &str, out: &Path) -> anyhow::Result<()> {
    let example = espforge_examples::find(example_path).context(format!(
        "no example at '{example_path}'; run `espforge examples list` to see what is available"
    ))?;
    let written = espforge_examples::materialize(example, out)
        .with_context(|| format!("failed to copy '{example_path}' into {}", out.display()))?;
    for path in written {
        println!("{}", path.display());
    }
    Ok(())
}

/// Human title from the example file's banner, empty when there is none.
fn example_hint(example: &Example) -> String {
    match &example.title {
        Some(title) => title.clone(),
        None => String::new(),
    }
}

/// Number of examples in the category, singular when there's exactly one.
fn category_hint(category: &Category) -> String {
    let noun = if category.examples.len() == 1 {
        "example"
    } else {
        "examples"
    };
    format!("{} {noun}", category.examples.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn categories_listing_works_and_counts_match_the_catalog() {
        assert!(run_categories().is_ok());
        for c in espforge_examples::categories() {
            assert!(
                category_hint(c).starts_with(&c.examples.len().to_string()),
                "{}",
                c.name
            );
        }
    }

    #[test]
    fn list_without_a_category_covers_every_example() {
        let total: usize = espforge_examples::categories()
            .iter()
            .map(|c| c.examples.len())
            .sum();
        assert_eq!(espforge_examples::list(None).unwrap().len(), total);
        assert!(run_list(None).is_ok());
        assert!(run_list(Some("no-such-category")).is_err());
    }

    #[test]
    fn the_materialized_example_builds_after_a_chip_change() {
        // examples -> edit -> build, chained by hand: the contract of cli.md.
        let tmp = tempfile::tempdir().unwrap();
        let ex = espforge_examples::find("01.Basics/helloworld").unwrap();
        let dest = tmp.path().join("hw");
        espforge_examples::materialize(ex, &dest).unwrap();
        let espf = dest.join(EXAMPLE_FILE);
        let text = std::fs::read_to_string(&espf).unwrap();
        std::fs::write(&espf, set_chip(&text, "esp32c6").unwrap()).unwrap();
        let out = dest.join("build");
        build::execute(&BuildArgs {
            file: Some(espf),
            out: out.clone(),
            stage: Stage::Generate,
            ..Default::default()
        })
        .unwrap();
        let cargo = std::fs::read_to_string(out.join("Cargo.toml")).unwrap();
        assert!(cargo.contains("\"esp32c6\""));
    }
}
