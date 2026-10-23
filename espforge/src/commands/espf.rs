//! `espforge espf` (alias `ref`): discover what an ESPF file can contain.
//!
//! The catalog path is the interface — there is no `list` or `show`
//! subcommand. At every level the bare command lists its children as
//! complete, copyable commands; only a leaf is different, where `help` is
//! the man page and `examples` the cheat sheet.
//!
//! Layering (cli.md): this file is I/O and paging only. Loading, path
//! resolution and every string produced live in `espforge-options::catalog`
//! and `crate::help_view`.
//!
//! Exit codes: 0 success, 1 user error (unknown name; already printed), 2
//! internal failure.

use clap::Args;
use espforge_options::catalog::{self, Match, ResolutionError, View};
use std::io::IsTerminal;
use std::process::ExitCode;
use std::process::{Command, Stdio};

#[derive(Args, Debug, Clone, Default)]
pub struct EspfArgs {
    /// `overview | section [leaf [help | examples]]`; bare lists everything
    pub path: Vec<String>,
    /// Never page long output (`overview`, leaf `help`)
    #[arg(long)]
    pub no_pager: bool,
}

pub fn run(args: EspfArgs) -> ExitCode {
    match execute(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(code) => ExitCode::from(code),
    }
}

fn execute(args: &EspfArgs) -> Result<(), u8> {
    let catalog = catalog::embedded();

    // Rule 1 of plan 3.3: a trailing `help` or `examples` is the view, not
    // part of the path, and is stripped before any lookup.
    let (path, view) = catalog::strip_view(args.path.clone());
    let found = catalog::resolve(catalog, &path);

    // Ok: output for the user. Err: a user error, printed and exit 1.
    let rendered = match &found {
        Ok(m) => render(m, view),
        Err(e) => Err(crate::help_view::render_resolution_error(e, catalog)),
    };

    let Ok(text) = rendered else {
        eprintln!("❌ {}", rendered.unwrap_err());
        return Err(1);
    };

    // Only the man page and the overview grow beyond the screen; the lists
    // and the cheat sheet are short and print plain.
    if pages_of(&found, &view) && may_page(args) {
        page(&text).map_err(|_| 2)
    } else {
        print!("{text}");
        Ok(())
    }
}

/// What each resolved path renders to (`Ok` = output, `Err` = user error). A
/// view a level does not offer errors (rule 4.2: `examples` exists only on a
/// leaf).
fn render(m: &Match, view: View) -> Result<String, String> {
    let piped = !stdout_is_terminal();
    match (m, view) {
        (Match::Root(c), View::Default | View::Help) => Ok(crate::help_view::render_root(c, piped)),
        (Match::Overview(c), View::Default | View::Help) => Ok(crate::help_view::render_overview(c)),
        (Match::Overview(_), View::Examples) => Err(examples_needs_a_leaf()),
        (Match::Section(s), View::Default | View::Help) => {
            Ok(crate::help_view::render_section(s, piped))
        }
        (Match::Leaf(e), View::Default) => Ok(crate::help_view::render_leaf_list(e, piped)),
        (Match::Leaf(e), View::Help) => Ok(crate::help_view::render_man(catalog::embedded(), e)),
        (Match::Leaf(e), View::Examples) => Ok(crate::help_view::render_examples(e)),
        (Match::Root(_) | Match::Section(_), View::Examples) => Err(examples_needs_a_leaf()),
    }
}

fn examples_needs_a_leaf() -> String {
    "examples works on an element, not a section.\n\
     Pick one: `espforge espf <section> <element> examples`.\n"
        .to_string()
}

fn pages_of(found: &Result<Match, ResolutionError>, view: &View) -> bool {
    match found {
        Ok(Match::Overview(_)) => true,
        Ok(Match::Leaf(_)) if *view == View::Help => true,
        _ => false,
    }
}

/// A manual page is allowed to page: on a terminal, into `$PAGER`
/// (default `less -R`), like `git help`. Piped, we always print plain.
fn may_page(args: &EspfArgs) -> bool {
    if args.no_pager {
        return false;
    }
    stdout_is_terminal()
}

fn stdout_is_terminal() -> bool {
    std::io::stdout().is_terminal()
}

/// Write `text` through the user's pager. The pager is found like git finds
/// it: `$PAGER` when set and resolvable, else `less -R` when present.
fn page(text: &str) -> std::io::Result<()> {
    let pager = std::env::var("PAGER").unwrap_or_else(|_| "less -R".to_string());
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(&pager)
        .stdin(Stdio::piped())
        .spawn()?;
    use std::io::Write;
    child
        .stdin
        .take()
        .expect("stdin was piped")
        .write_all(text.as_bytes())?;
    child.wait()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exec(path: &[&str], no_pager: bool) -> Result<(), u8> {
        execute(&EspfArgs {
            path: path.iter().map(|s| s.to_string()).collect(),
            no_pager,
        })
    }

    #[test]
    fn the_root_lists_everything() {
        assert_eq!(exec(&[], false), Ok(()));
    }

    #[test]
    fn every_path_in_the_grammar_works() {
        assert_eq!(exec(&["overview"], true), Ok(()));
        assert_eq!(exec(&["peripherals"], true), Ok(()));
        assert_eq!(exec(&["peripherals", "help"], true), Ok(()));
        assert_eq!(exec(&["peripherals", "i2c"], true), Ok(()));
        assert_eq!(exec(&["peripherals", "i2c", "help"], true), Ok(()));
        assert_eq!(exec(&["peripherals", "i2c", "examples"], true), Ok(()));
        // Shorthand: `i2c` is unique across the catalog.
        assert_eq!(exec(&["i2c"], true), Ok(()));
    }

    #[test]
    fn unknown_names_are_user_errors() {
        assert_eq!(exec(&["peripherals", "i21"], true), Err(1));
        assert_eq!(exec(&["nosuch"], true), Err(1));
        assert_eq!(exec(&["peripherals", "noleaf", "help"], true), Err(1));
    }

    #[test]
    fn examples_outside_a_leaf_is_an_error_that_says_so() {
        // A section followed by `examples`: the strip removes the word, the
        // view check refuses it, and the message says an element is needed.
        assert_eq!(exec(&["peripherals", "examples"], true), Err(1));
    }

    #[test]
    fn overview_takes_no_further_arguments() {
        assert_eq!(exec(&["overview", "peripherals"], true), Err(1));
        assert_eq!(exec(&["overview", "a", "b", "c", "d"], true), Err(1));
    }

    /// Drift test 11 of the plan: the sample file shown in `overview.txt`
    /// passes the same validation `check` uses (everything except the
    /// buildable check), so the overview can never show a file espforge
    /// rejects.
    #[test]
    fn the_overview_sample_penetrates_check_validation() {
        use crate::commands::answers::AnswerArgs;
        use crate::commands::pipeline;

        let overview = catalog::embedded().overview;
        let start = overview
            .find("EXAMPLE\n")
            .expect("overview has an EXAMPLE block")
            + "EXAMPLE\n".len();
        let end = overview
            .find("\nNEXT\n")
            .expect("overview has a NEXT block");
        let body = &overview[start..end];

        let indent = body
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| l.len() - l.trim_start().len())
            .min()
            .unwrap_or(0);
        let sample: String = body
            .lines()
            .map(|l| l.get(indent..).unwrap_or("").to_string())
            .collect::<Vec<_>>()
            .join("\n");

        let checked = pipeline::validate(&sample, &AnswerArgs::default())
            .map_err(|invalid| invalid.report("overview-sample", &sample))
            .expect("the overview sample must be valid ESPF");
        assert_eq!(checked.ctx.resolved_devices.len(), 0);
    }

    /// Drift test 8 of the plan: every driver in the bindings registry has a
    /// `devices` entry in the catalog, and has status implemented (it builds
    /// today).
    #[test]
    fn every_routed_driver_has_a_devices_entry() {
        use espforge_bindings::drivers::generated_registry;

        let c = catalog::embedded();
        for tag in generated_registry::TAGS {
            let device = c
                .leafs_by_tag(tag)
                .into_iter()
                .find(|e| e.section == "devices")
                .unwrap_or_else(|| panic!("driver `{tag}` has no `devices` entry"));
            assert_eq!(device.status.marker(), "", "`{tag}` builds, so no marker");
        }
    }

    /// Drift test 9 of the plan: every component wrapper has a `components`
    /// entry whose required attributes match the wrapper's. `id` is required
    /// by the parser for every element, so it is excluded on both sides.
    #[test]
    fn component_wrappers_agree_with_the_catalog() {
        use espforge_bindings::components;

        let c = catalog::embedded();
        for tag in components::TAGS {
            let wrapper = components::resolve_tag_required_attrs(tag)
                .unwrap_or_else(|| panic!("no wrapper module for `{tag}`"));
            let def = c
                .leafs_by_tag(tag)
                .into_iter()
                .find(|e| e.section == "components")
                .unwrap_or_else(|| panic!("wrapper `{tag}` has no `components` entry"));
            let mut wrapper: Vec<&str> = wrapper;
            wrapper.sort();
            let mut catalog_required: Vec<&str> = def
                .attrs
                .iter()
                .filter(|a| a.required && a.name != "id")
                .map(|a| a.name.as_str())
                .collect();
            catalog_required.sort();
            assert_eq!(catalog_required, wrapper, "required attributes of `{tag}`");
        }
    }
}
