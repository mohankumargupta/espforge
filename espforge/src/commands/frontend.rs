//! The front half of the pipeline, shared by `check` and `build`:
//! parse -> syntax + structure errors -> `# espforge` core -> `<options>`.

use espf::diag::EspfError;
use espf::parser::Parsed;
use espforge_diagnostics::{Diagnostic, to_human};
use espforge_model::Meta;
use espforge_options::{Answers, DerivedInputs, core_from_section, embedded_schema};

/// A parsed, structurally valid ESPF file.
pub struct Frontend<'a> {
    pub parsed: Parsed<'a>,
    /// Core settings exactly as written in the file (before any flags).
    pub meta: Meta,
    /// Explicit option answers exactly as written in the file.
    pub answers: Answers,
    pub derived: DerivedInputs,
}

/// Run every check that needs only the file. All problems of the failing
/// stage are returned together, sorted by position.
pub fn load(src: &str) -> Result<Frontend<'_>, Vec<EspfError>> {
    let parsed = espf::parser::parse_espf(src);

    // 1. Syntax (junk lines) and structure (duplicate / missing sections).
    let mut errors = espf::diag::junk_errors(&parsed.junk);
    errors.extend(parsed.structure_errors.iter().cloned());
    if !errors.is_empty() {
        errors.sort_by_key(|e| e.span.start);
        return Err(errors);
    }

    // 2. `# espforge` core and `<options>` (all five sections exist now).
    let header = parsed.header_offsets["espforge"];
    let espforge_nodes = &parsed.sections["espforge"];
    let (meta, opt_node) = core_from_section(header, espforge_nodes)?;
    let answers = Answers::from_node(embedded_schema(), opt_node, header..header)?;
    let derived = DerivedInputs::from_nodes(espforge_nodes, &parsed.sections["services"]);

    Ok(Frontend {
        parsed,
        meta,
        answers,
        derived,
    })
}

/// Print each diagnostic as a snippet block, then a one-line summary.
pub fn report(name: &str, src: &str, diagnostics: &[Diagnostic]) {
    eprintln!("{}", to_human(name, src, diagnostics));
    eprintln!(
        "❌ Found {} problem(s) in '{}'; fix them and re-run.",
        diagnostics.len(),
        name
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    const OK: &str = "# espforge\n  <project name=\"hello\" />\n  <chip type=\"esp32c3\" />\n  <runtime type=\"blocking\" />\n  <options defaults=\"false\" />\n# peripherals\n# components\n# devices\n# services\n";

    #[test]
    fn loads_a_valid_file() {
        let fe = load(OK).unwrap();
        assert_eq!(fe.meta.chip, "esp32c3");
        assert!(!fe.answers.defaults);
    }

    #[test]
    fn missing_sections_are_reported_before_anything_else() {
        let errs = load("# espforge\n  <project name=\"h\" />\n")
            .err()
            .unwrap();
        assert!(
            errs.iter()
                .any(|e| e.title.contains("missing section `# services`"))
        );
    }

    #[test]
    fn core_errors_are_reported() {
        let src = OK.replace("esp32c3", "esp99");
        let errs = load(&src).err().unwrap();
        assert_eq!(errs[0].title, "unknown chip");
    }

    #[test]
    fn option_errors_are_reported() {
        let src = OK.replace("defaults=\"false\"", "wombat=\"1\"");
        let errs = load(&src).err().unwrap();
        assert!(errs[0].title.contains("unknown option `wombat`"));
    }
}
