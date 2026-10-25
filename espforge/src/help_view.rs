//! Rendering for `espforge espf`: every view is a pure function returning a
//! `String`. `commands/espf.rs` prints or pages the result.
//!
//! Query path: `catalog` (options) -> `help_view` (strings) -> `espf.rs`
//! (print/page).
//!
//! The uniform output shape is plan 3.2: a bare command lists its children as
//! complete, copyable commands — on a terminal in aligned columns, piped as
//! `command<TAB>description` (plan 11.5: the section's description paragraph
//! is omitted when piped, so piped output is only rows).
//!
//! Nothing here detects the terminal or touches I/O; the command layer passes
//! `piped` in.

use espforge_options::catalog::{
    AttrDef, AttrKind, Catalog, ElementDef, ResolutionError, SectionDef,
};

/// The command prefix every row starts with.
const ROOT_CMD: &str = "espforge espf";

const OVERVIEW_ROW: &str = "what an ESPF file is and how it is laid out";

// --------------------------------------------------------------------- lists

/// The one function behind the root, section and leaf lists: aligned columns
/// on a terminal, tab-separated when piped. Every row is a complete command.
pub fn render_columns(rows: &[(String, String)], piped: bool) -> String {
    let mut out = String::new();
    if piped {
        for (cmd, desc) in rows {
            out.push_str(&format!("{cmd}\t{desc}\n"));
        }
        return out;
    }
    let width = rows
        .iter()
        .map(|(cmd, _)| cmd.chars().count())
        .max()
        .unwrap_or(0);
    for (cmd, desc) in rows {
        out.push_str(&format!("{cmd:<width$}  {desc}\n"));
    }
    out
}

/// `espforge espf`: the sections, then `overview`.
pub fn render_root(c: &Catalog, piped: bool) -> String {
    let mut rows: Vec<(String, String)> = c
        .sections
        .iter()
        .map(|s| (format!("{ROOT_CMD} {}", s.name), s.summary.clone()))
        .collect();
    rows.push((format!("{ROOT_CMD} overview"), OVERVIEW_ROW.to_string()));
    render_columns(&rows, piped)
}

/// `espforge espf <section>`: the description paragraph (terminal only), then
/// the section's leafs.
pub fn render_section(s: &SectionDef, piped: bool) -> String {
    let mut out = String::new();
    if !piped {
        out.push_str(&s.description);
        out.push_str("\n\n");
    }
    let rows: Vec<(String, String)> = s
        .leafs
        .iter()
        .map(|e| {
            (
                format!("{ROOT_CMD} {} {}", s.name, e.tag),
                format!("{}{}", e.summary, e.status.marker()),
            )
        })
        .collect();
    out.push_str(&render_columns(&rows, piped));
    out
}

/// `espforge espf <section> <leaf>`: a one-line title, then the leaf's own
/// subcommands (`help`, `examples`) as complete commands (plan 11.4).
pub fn render_leaf_list(e: &ElementDef, piped: bool) -> String {
    let path = format!("{ROOT_CMD} {} {}", e.section, e.tag);
    let rows = vec![
        (
            format!("{path} help"),
            "man page: description, attributes, see also".to_string(),
        ),
        (
            format!("{path} examples"),
            "copy-ready snippets and example projects".to_string(),
        ),
    ];
    format!(
        "{} - {}\n{}",
        e.tag,
        e.summary,
        render_columns(&rows, piped)
    )
}

// ----------------------------------------------------------- leaf views

/// `espforge espf <section> <leaf> help`: the man page. Blocks with no data
/// are skipped, so a leaf can ship with only the generated parts (name,
/// synopsis, attributes, referenced by) and gain prose later.
pub fn render_man(c: &Catalog, e: &ElementDef) -> String {
    let mut out = String::new();

    out.push_str("NAME\n");
    out.push_str(&format!("    {} - {}\n", e.tag, e.summary));

    if let Some(synopsis) = render_synopsis(e) {
        out.push_str("\nSYNOPSIS\n");
        for line in synopsis {
            out.push_str(&format!("    {line}\n"));
        }
    }

    if let Some(description) = &e.description {
        out.push_str("\nDESCRIPTION\n");
        for paragraph in description.split('\n') {
            out.push_str("    ");
            out.push_str(paragraph.trim());
            out.push_str("\n");
        }
    }

    if !e.attrs.is_empty() {
        out.push_str("\nATTRIBUTES\n");
        out.push_str(&render_attribute_table(e));
    }

    let referenced = c.referenced_by(&e.tag);
    if !referenced.is_empty() {
        out.push_str("\nREFERENCED BY\n");
        let width = referenced
            .iter()
            .map(|(r, _)| r.heading().chars().count())
            .max()
            .unwrap_or(0);
        for (r, via) in referenced {
            out.push_str(&format!(
                "    {:<width$}   via {via}=\"$id\"\n",
                r.heading()
            ));
        }
    }

    if !e.see_also.is_empty() {
        out.push_str("\nSEE ALSO\n");
        for target in &e.see_also {
            // `components/i2c_component` becomes a real command.
            let path = target.replace('/', " ");
            out.push_str(&format!("    {ROOT_CMD} {path}\n"));
        }
        out.push_str(&format!(
            "    {ROOT_CMD} {} {} examples\n",
            e.section, e.tag
        ));
    }

    out
}

/// `SYNOPSIS` lines: required attributes as `name="VALUE"`, optional ones in
/// `[...]`, like the plan's `<i2c id="..." sda="PIN" ... [frequency="FREQ"] />`.
fn render_synopsis(e: &ElementDef) -> Option<Vec<String>> {
    if e.attrs.is_empty() {
        return None;
    }
    let mut line = String::new();
    line.push('<');
    line.push_str(&e.tag);
    line.push(' ');
    let parts: Vec<String> = e
        .attrs
        .iter()
        .map(|a| {
            let part = format!("{}={}", a.name, placeholder(&a.kind));
            if a.required {
                part
            } else {
                format!("[{part}]")
            }
        })
        .collect();
    line.push_str(&parts.join(" "));
    line.push_str(" />");
    Some(vec![line])
}

/// The placeholder for one attribute's value in a synopsis.
fn placeholder(kind: &AttrKind) -> String {
    match kind {
        AttrKind::Ident => "\"...\"".to_string(),
        AttrKind::Str => "\"VALUE\"".to_string(),
        AttrKind::Pin => "\"PIN\"".to_string(),
        AttrKind::Freq => "\"FREQ\"".to_string(),
        AttrKind::Hex => "\"ADDR\"".to_string(),
        AttrKind::U32 => "\"N\"".to_string(),
        AttrKind::Bool => "\"true\"".to_string(),
        AttrKind::Choice(cs) => cs
            .first()
            .map(|c| format!("\"{c}\""))
            .unwrap_or_else(|| "\"\"".to_string()),
        AttrKind::Ref { to } => {
            format!("\"${}\"", to.first().map(String::as_str).unwrap_or("id"))
        }
        AttrKind::PinOrRef => "\"PIN\"".to_string(),
        AttrKind::List(inner) => format!("\"{}+=\"", placeholder(inner)),
    }
}

/// The `ATTRIBUTES` block: adjacent attributes of the same shape share a row
/// (`sda, scl    (required, pin)`), required/optional, kind and default
/// between the parentheses, the help text after.
fn render_attribute_table(e: &ElementDef) -> String {
    let rows = attribute_rows(e);
    let width = rows
        .iter()
        .map(|(names, _)| names.chars().count())
        .max()
        .unwrap_or(0);
    let mut out = String::new();
    for (names, text) in rows {
        out.push_str(&format!("    {:<width$} {}\n", names, text));
    }
    out
}

fn attribute_rows(e: &ElementDef) -> Vec<(String, String)> {
    let mut rows: Vec<(String, String)> = Vec::new();
    let mut group: Vec<&AttrDef> = Vec::new();

    // Same shape groups `sda, scl (required, pin)` even when only one of
    // them carries help text (the first one's is shown).
    let same_shape = |a: &AttrDef, b: &AttrDef| {
        a.kind.display() == b.kind.display() && a.required == b.required && a.default == b.default
    };

    for attr in &e.attrs {
        match group.last() {
            Some(last) if same_shape(last, attr) => group.push(attr),
            Some(_) => {
                push_group(&group, &mut rows);
                group.clear();
                group.push(attr);
            }
            None => group.push(attr),
        }
    }
    if !group.is_empty() {
        push_group(&group, &mut rows);
    }
    rows
}

fn push_group(group: &[&AttrDef], rows: &mut Vec<(String, String)>) {
    let names = group
        .iter()
        .map(|a| a.name.clone())
        .collect::<Vec<_>>()
        .join(", ");
    let first = group[0];
    let mut shape = if first.required {
        "required".to_string()
    } else {
        "optional".to_string()
    };
    shape.push_str(", ");
    shape.push_str(&first.kind.display());
    if let Some(default) = &first.default {
        shape.push_str(&format!(", default {default}"));
    }
    let help = first.help.as_deref().unwrap_or("");
    let text = if help.is_empty() {
        format!("({shape})")
    } else {
        format!("({shape}) {help}")
    };
    rows.push((names, text));
}

/// `espforge espf <section> <leaf> examples`: the cheat sheet (plan 4.4).
/// Snippets come straight from the catalog, dedented; the title is the
/// trailing `// comment` the ESPF grammar allows, aligned across snippets and
/// pasted onto the snippet's first element line, so everything printed is
/// valid ESPF. Never paged — the command layer decides.
pub fn render_examples(e: &ElementDef) -> String {
    // (lines of the dedented fragment, index of its first element line)
    let snippets: Vec<(Vec<String>, usize)> = e
        .examples
        .iter()
        .filter_map(|ex| {
            let lines: Vec<String> = dedent(&ex.fragment).lines().map(str::to_string).collect();
            lines
                .iter()
                .position(|l| is_element_line(l))
                .map(|i| (lines, i))
        })
        .collect();

    let label_col = snippets
        .iter()
        .map(|(lines, i)| lines[*i].chars().count())
        .max()
        .unwrap_or(0)
        + 2;

    let mut out = String::new();
    for (n, (lines, first_element)) in snippets.iter().enumerate() {
        if n > 0 {
            out.push('\n');
        }
        for (k, line) in lines.iter().enumerate() {
            if k == *first_element {
                let title = &e.examples[n].title;
                out.push_str(&format!("{line:<label_col$}// {title}\n"));
            } else {
                out.push_str(line);
                out.push('\n');
            }
        }
    }
    out
}

/// Remove the common leading indentation of a fragment (KDL write style
/// indents every fragment line inside espf.schema.kdl).
fn dedent(fragment: &str) -> String {
    let lines: Vec<&str> = fragment.lines().collect();
    let indent = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .min()
        .unwrap_or(0);
    lines
        .iter()
        .map(|l| l.get(indent..).unwrap_or(l))
        .collect::<Vec<_>>()
        .join("\n")
}

/// A line an example label can be pasted onto: a real element line, not a
/// `# section` header, not a blank, not already carrying a comment.
fn is_element_line(line: &str) -> bool {
    let t = line.trim();
    !t.is_empty() && !t.starts_with('#') && !t.contains("//")
}

/// `espforge espf overview`: the preface prose from `overview.txt`.
pub fn render_overview(c: &Catalog) -> String {
    format!("{}\n", c.overview.trim_end())
}

// -------------------------------------------------------------------- errors

/// Why the path did not resolve, with the catalog in hand for "did you mean"
/// suggestions (plan 3.3 rules 4-5).
pub fn render_resolution_error(e: &ResolutionError, c: &Catalog) -> String {
    match e {
        ResolutionError::Unknown { text, suggestions } => {
            let mut out = format!("no element or section named `{text}`\n");
            match suggestions.as_slice() {
                [] => {}
                [one] => out.push_str(&format!("\ndid you mean `{one}`?\n")),
                many => {
                    let list: Vec<String> = many.iter().map(|s| format!("`{s}`")).collect();
                    out.push_str(&format!("\ndid you mean {}?\n", list.join(" ")));
                }
            }
            out.push_str(&section_list(c));
            out
        }
        ResolutionError::Ambiguous { tag, candidates } => {
            let mut out = format!("`{tag}` names an element in more than one section:\n");
            for candidate in candidates {
                out.push_str(&format!("    {ROOT_CMD} {candidate}\n"));
            }
            out.push_str("\nUse one of those forms.\n");
            out
        }
        ResolutionError::WrongSection { tag, section } => {
            format!("`{tag}` is in `{section}`.\n\ntry `{ROOT_CMD} {section} {tag}`\n")
        }
        ResolutionError::TooManyArgs { args } => {
            let given = args.join(" ");
            format!(
                "too much after `{given}`\n\nusage: {ROOT_CMD} [overview | section [leaf [help | examples]]]\n"
            )
        }
    }
}

/// The `sections:` footer in an unknown-name error.
fn section_list(c: &Catalog) -> String {
    let mut out = String::from("\nsections:\n");
    for s in &c.sections {
        out.push_str(&format!("    {}\n", s.name));
    }
    out.push_str("    overview\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use espforge_options::catalog;

    fn leaf(tag: &str) -> ElementDef {
        catalog::embedded()
            .sections
            .iter()
            .flat_map(|s| s.leafs.iter())
            .find(|e| e.tag == tag)
            .unwrap()
            .clone()
    }

    fn catalog() -> &'static Catalog {
        catalog::embedded()
    }

    #[test]
    fn the_root_lists_sections_then_overview() {
        let out = render_root(catalog(), false);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), catalog().sections.len() + 1);
        assert!(lines[0].starts_with("espforge espf espforge"), "{lines:?}");
        assert!(
            lines.last().unwrap().starts_with("espforge espf overview"),
            "{lines:?}"
        );
        // Piped: same rows, `command<TAB>description`, no padding.
        let piped = render_root(catalog(), true);
        assert_eq!(piped.lines().count(), lines.len());
        assert!(piped.lines().all(|l| l.matches('\t').count() == 1));
        assert!(!out.contains('\t'));
    }

    #[test]
    fn a_section_shows_description_then_leafs() {
        let peripherals = catalog().section_named("peripherals").unwrap();
        let out = render_section(peripherals, false);
        assert!(
            out.starts_with("The chip's hardware blocks that your project uses."),
            "\n{out}"
        );
        assert!(out.contains("espforge espf peripherals i2c"), "\n{out}");
        assert!(out.contains("I2C bus controller"), "\n{out}");
        // Piped: description omitted (plan 11.5); rows only.
        let piped = render_section(peripherals, true);
        assert!(!piped.contains("hardware blocks that"), "\n{piped}");
        assert_eq!(piped.lines().count(), peripherals.leafs.len());
    }

    #[test]
    fn planned_leafs_carry_the_marker() {
        let services = catalog().section_named("services").unwrap();
        let out = render_section(services, true);
        assert!(out.contains(" (planned)"), "\n{out}");
    }

    #[test]
    fn a_leaf_lists_help_and_examples_under_a_title() {
        let out = render_leaf_list(&leaf("i2c"), false);
        assert!(out.starts_with("i2c - I2C bus controller\n"), "\n{out}");
        assert!(
            out.contains("espforge espf peripherals i2c help"),
            "\n{out}"
        );
        assert!(out.contains("man page:"), "\n{out}");
        assert!(
            out.contains("espforge espf peripherals i2c examples"),
            "\n{out}"
        );
        assert!(out.contains("copy-ready snippets"), "\n{out}");
    }

    #[test]
    fn the_man_page_has_name_synopsis_attributes_and_references() {
        let out = render_man(catalog(), &leaf("i2c"));
        assert!(
            out.starts_with("NAME\n    i2c - I2C bus controller\n"),
            "\n{out}"
        );
        assert!(
            out.contains("<i2c id=\"...\" sda=\"PIN\" scl=\"PIN\" [frequency=\"FREQ\"] />"),
            "\n{out}"
        );
        assert!(out.contains("(required, ident)"), "\n{out}");
        assert!(out.contains("(required, pin)"), "\n{out}");
        assert!(out.contains("(optional, freq, default 100kHz)"), "\n{out}");
        assert!(out.contains("components/i2c_component"), "\n{out}");
        assert!(out.contains("via i2c=\"$id\""), "\n{out}");
        assert!(
            out.contains("espforge espf components i2c_component"),
            "\n{out}"
        );
        assert!(
            out.contains("espforge espf peripherals i2c examples"),
            "\n{out}"
        );
    }

    #[test]
    fn the_cheat_sheet_labels_snippets_with_comments() {
        let out = render_examples(&leaf("i2c"));
        assert!(
            out.contains("<i2c id=\"i2c0\" sda=\"6\" scl=\"7\" />"),
            "\n{out}"
        );
        assert!(out.contains("// minimal"), "\n{out}");
        assert!(out.contains("// with a bus speed"), "\n{out}");
        assert!(out.contains("// with a component"), "\n{out}");
        assert!(out.contains("# peripherals"), "\n{out}");
    }

    #[test]
    fn an_element_without_description_skips_the_block() {
        let out = render_man(catalog(), &leaf("uart"));
        assert!(!out.contains("DESCRIPTION"), "\n{out}");
        assert!(out.contains("NAME"), "\n{out}");
    }

    #[test]
    fn unknown_names_list_suggestions_and_sections() {
        let err = ResolutionError::Unknown {
            text: "i21".to_string(),
            suggestions: vec!["i2c".to_string()],
        };
        let out = render_resolution_error(&err, catalog());
        assert!(out.contains("no element or section named `i21`"), "\n{out}");
        assert!(out.contains("did you mean `i2c`?"), "\n{out}");
        assert!(out.contains("components"), "\n{out}");
    }

    #[test]
    fn the_wrong_section_error_names_where_to_look() {
        let err = ResolutionError::WrongSection {
            tag: "i2c".to_string(),
            section: "peripherals".to_string(),
        };
        let out = render_resolution_error(&err, catalog());
        assert!(out.contains("`i2c` is in `peripherals`"), "\n{out}");
        assert!(out.contains("espforge espf peripherals i2c"), "\n{out}");
    }

    #[test]
    fn dedent_removes_the_common_indent() {
        let f = "    # peripherals\n      <i2c id=\"i2c0\" sda=\"6\" scl=\"7\" />\n";
        assert_eq!(
            dedent(f),
            "# peripherals\n  <i2c id=\"i2c0\" sda=\"6\" scl=\"7\" />"
        );
    }
}
