use crate::context::{AttrOp, ParsedNode, TagAttribute};
use crate::diag::{EspfError, JunkKind, JunkSpan};
use std::collections::HashMap;
use winnow::{
    ModalResult, Parser,
    ascii::{multispace0, multispace1, till_line_ending},
    combinator::{alt, dispatch, fail, opt, peek, preceded},
    stream::Stream,
    token::{any, take_while},
};

/// The five sections every ESPF file must contain (empty sections are valid).
pub const REQUIRED_SECTIONS: [&str; 5] = [
    "espforge",
    "peripherals",
    "components",
    "devices",
    "services",
];

/// Everything the parser learned about a file.
pub struct Parsed<'a> {
    /// Section name -> its elements, in source order. For a duplicated
    /// section the first definition wins.
    pub sections: HashMap<String, Vec<ParsedNode<'a>>>,
    /// Lines the parser could not interpret (see `diag::junk_errors`).
    pub junk: Vec<JunkSpan>,
    /// Byte offset of each section's `#` header (first definition).
    pub header_offsets: HashMap<String, usize>,
    /// Located structural errors: duplicate and missing sections.
    pub structure_errors: Vec<EspfError>,
}

/// Identifier-like token: letters, digits, `_`, `-`.
fn identifier<'a>(input: &mut &'a str) -> ModalResult<&'a str> {
    take_while(1.., |c: char| {
        c.is_ascii_alphanumeric() || c == '_' || c == '-'
    })
    .parse_next(input)
}

/// Attribute key: one or more identifiers joined by `.` (`ssid`, `network1.ssid`,
/// `a.b.c`). Returns the whole dotted key as one slice. No structure is
/// checked here; `value::assemble` turns dotted keys into objects.
fn dotted_key<'a>(input: &mut &'a str) -> ModalResult<&'a str> {
    (
        identifier,
        winnow::combinator::repeat(0.., preceded('.', identifier)).map(|_: Vec<&str>| ()),
    )
        .take()
        .parse_next(input)
}

/// One comment block: an opening line starting with one or more `=`, one or
/// more comment lines, and a closing line made only of `=`. The first line is
/// already positioned at the opening `=` (skipped dispatch matched it), so
/// consuming until the line ending swallows `===...  title text` too.
fn block_comment(input: &mut &str) -> ModalResult<()> {
    // Opening line: `=` sigils plus any title text, then its newline.
    till_line_ending.void().parse_next(input)?;
    consume_newline(input)?;

    // Body lines. A line whose trimmed content is only `=` closes the block;
    // anything else is a comment line, including blank lines.
    while !input.is_empty() {
        let line_end = input.find(['\r', '\n']).map_or(input.len(), |i| i);
        let is_closing = !input[..line_end].trim_end().is_empty()
            && input[..line_end].trim_end().chars().all(|c| c == '=');

        *input = &input[line_end..];
        consume_newline(input)?;

        if is_closing {
            break;
        }
    }
    // An unterminated block (EOF before the closing `=` line) is tolerated.

    Ok(())
}

/// Skip blank lines and `=`-delimited comment blocks.
///
/// Uses `opt(dispatch!{...})` so "not trivia" cleanly stops the loop
/// without producing an error, and so the error type is pinned by
/// `ModalResult`.
fn skip_trivia(input: &mut &str) -> ModalResult<()> {
    loop {
        let before = *input;

        let _ = opt(dispatch! {peek(any);
            '=' => block_comment,
            ' ' | '\t' | '\r' | '\n' => multispace1.void(),
            _ => fail,
        })
        .parse_next(input)?;

        if *input == before {
            break;
        }
    }
    Ok(())
}

/// The amount of input consumed so far, relative to the start of the file.
fn consumed_since(input: &str, original_start: &str) -> usize {
    original_start.len() - input.len()
}

/// Section header: `# name`
fn section_header<'a>(input: &mut &'a str) -> ModalResult<&'a str> {
    preceded(('#', multispace0), identifier).parse_next(input)
}

/// Everything up to the closing `"`. Values may not span lines: a newline is
/// not an acceptable terminator, so an unterminated quote fails the attribute
/// (and thus the line) instead of reaching the next line's `"`.
fn unquoted_value<'a>(input: &mut &'a str) -> ModalResult<&'a str> {
    take_while(0.., |c: char| c != '"' && c != '\n' && c != '\r').parse_next(input)
}

/// `key="value"` or `key+="value"`, where `key` may be dotted.
///
/// Records the byte offsets of both the key and the value in the original
/// file for span-aware diagnostics. A missing closing `"` fails the attribute
/// (and thus the line). The parser stays lossless: it reports the raw
/// (key, op, value) and leaves structural validation to `value::assemble`.
fn attribute<'a>(input: &mut &'a str, file: &str) -> ModalResult<TagAttribute<'a>> {
    let key_offset = consumed_since(input, file);
    let key = dotted_key.parse_next(input)?;
    // `+=` must be tried first: `=` alone would leave a stray `+` behind.
    let op = alt(("+=".value(AttrOp::Append), "=".value(AttrOp::Set))).parse_next(input)?;
    let _ = '"'.void().parse_next(input)?;
    let val_offset = consumed_since(input, file);
    let val = unquoted_value.parse_next(input)?;
    let _ = '"'.void().parse_next(input)?;
    Ok(TagAttribute {
        key,
        op,
        key_offset,
        val,
        val_offset,
    })
}

/// Self-closing tag: `<tag attr="v" ... />`
fn parse_node<'a>(input: &mut &'a str, file: &str) -> ModalResult<ParsedNode<'a>> {
    preceded(
        (multispace0, '<'),
        (
            // Record the byte offset of the tag start for diagnostics:
            // exactly one byte (`<`) has been consumed here.
            move |input: &mut &'a str| {
                let tag_offset = consumed_since(input, file).saturating_sub(1);
                identifier.parse_next(input).map(|t| (t, tag_offset))
            },
            winnow::combinator::repeat(
                0..,
                preceded(multispace1, move |input: &mut &'a str| {
                    attribute(input, file)
                }),
            ),
            preceded(multispace0, "/>"),
        ),
    )
    .map(
        |(tag, attributes, _): ((&str, usize), Vec<TagAttribute>, &str)| {
            let (tag_name, tag_offset) = tag;
            let id = attributes
                .iter()
                .find(|a| a.key == "id" && a.op == AttrOp::Set)
                .map(|a| a.val)
                .unwrap_or("");
            ParsedNode {
                offset: tag_offset,
                tag_name,
                id,
                attributes,
            }
        },
    )
    .parse_next(input)
}

/// Consume exactly one line terminator (`\r\n`, `\n`, or `\r`) if present.
/// Returns `Ok(true)` if anything was consumed.
///
/// Order matters: check `\r\n` before `\n` and `\r`.
fn consume_newline(input: &mut &str) -> ModalResult<bool> {
    for nl in ["\r\n", "\n", "\r"] {
        if let Some(rest) = input.strip_prefix(nl) {
            *input = rest;
            return Ok(true);
        }
    }
    Ok(false)
}

/// Parse the contents of a single section, up to (but not consuming) the next
/// section header or EOF.
///
/// `junk_spans` collects byte offsets and line numbers of lines that did not
/// look like `<tag attr="v" />` and were skipped as junk. Callers that care
/// about nice error messages can render them with `annotate-snippets`.
fn section_content<'a>(
    input: &mut &'a str,
    file: &'a str,
    junk_spans: &mut Vec<JunkSpan>,
) -> ModalResult<Vec<ParsedNode<'a>>> {
    let mut nodes = Vec::new();

    loop {
        // 1. Skip whitespace / comments.
        skip_trivia.parse_next(input)?;

        // 2. Stop at EOF.
        if input.is_empty() {
            break;
        }

        // 3. Stop if the next thing is a section header (peek, don't consume).
        if peek(('#', multispace0, identifier))
            .parse_next(input)
            .is_ok()
        {
            break;
        }

        // 4. Try to parse a self-closing tag.
        //
        // On failure winnow may have partially consumed `input`, so reset to
        // the checkpoint first: the junk offset must point at the *start* of
        // the offending line.
        let checkpoint = input.checkpoint();
        if let Ok(node) = parse_node(input, file) {
            // A leaf node must stand on its own line: after `/>` only blanks
            // may remain before the newline/EOF, except for a trailing
            // inline `//` comment, which is allowed directly after the
            // self-closing element (and nowhere else).
            let first_non_blank = input
                .find(|c: char| c != ' ' && c != '\t')
                .unwrap_or(input.len());
            let tail = &input[first_non_blank..];
            let is_whole_line = tail.is_empty() || tail.starts_with('\r') || tail.starts_with('\n');
            if is_whole_line {
                nodes.push(node);
                continue;
            }
            if tail.starts_with("//") {
                let _ = till_line_ending.parse_next(input)?;
                let _ = consume_newline(input)?;
                nodes.push(node);
                continue;
            }
        }
        input.reset(&checkpoint);

        // 5. Junk line: record where it starts, then consume one full line.
        let junk_offset = consumed_since(input, file);
        junk_spans.push(JunkSpan {
            offset: junk_offset,
            line: crate::diag::line_number_for_offset(file, junk_offset),
            kind: JunkKind::NodeLine,
        });

        let _ = till_line_ending.parse_next(input)?;
        let _ = consume_newline(input)?;
    }

    Ok(nodes)
}

/// Parse a whole ESPF file.
fn parse_impl<'a>(mut input: &'a str, file: &'a str) -> Parsed<'a> {
    let mut junk_spans = Vec::new();
    let mut header_offsets: HashMap<String, usize> = HashMap::new();
    let mut structure_errors = Vec::new();
    let mut sections: HashMap<String, Vec<ParsedNode<'a>>> = HashMap::new();

    skip_trivia.parse_next(&mut input).ok();

    while !input.is_empty() {
        skip_trivia.parse_next(&mut input).ok();
        if input.is_empty() {
            break;
        }

        // Byte offset of the start of this section header in `file`.
        let header_offset = consumed_since(input, file);

        let section_name = match section_header.parse_next(&mut input) {
            Ok(n) => n,
            Err(_) => {
                // Malformed section header: record and skip the whole line so
                // we can keep going and report *all* problems, not just the
                // first one.
                junk_spans.push(JunkSpan {
                    offset: header_offset,
                    line: crate::diag::line_number_for_offset(file, header_offset),
                    kind: JunkKind::SectionHeader,
                });
                let _: ModalResult<&str> = till_line_ending.parse_next(&mut input);
                let _ = consume_newline(&mut input);
                continue;
            }
        };
        // End of `# name`, for underlining a duplicate header.
        let header_end = consumed_since(input, file);

        skip_trivia.parse_next(&mut input).ok();

        match section_content(&mut input, file, &mut junk_spans) {
            Ok(nodes) => {
                if let Some(&first) = header_offsets.get(section_name) {
                    // Keep the first definition; report the repeat.
                    structure_errors.push(EspfError {
                        title: format!("duplicate section `# {section_name}`"),
                        label: format!(
                            "`# {section_name}` is already defined on line {}; merge the two sections",
                            crate::diag::line_number_for_offset(file, first)
                        ),
                        span: header_offset..header_end,
                    });
                } else {
                    header_offsets.insert(section_name.to_string(), header_offset);
                    sections.insert(section_name.to_string(), nodes);
                }
            }
            Err(_) => {
                // section_content basically never errors (junk lines are
                // always consumed), but if it does, skip to the next line.
                let _: ModalResult<&str> = till_line_ending.parse_next(&mut input);
                let _ = consume_newline(&mut input);
            }
        }
    }

    // Every required section must be present. Point at the last section
    // header seen (or the top of the file when there is none).
    let anchor = header_offsets.values().copied().max().unwrap_or(0);
    for name in REQUIRED_SECTIONS {
        if !sections.contains_key(name) {
            structure_errors.push(EspfError {
                title: format!("missing section `# {name}`"),
                label: format!("every ESPF file needs a `# {name}` section (it may be empty)"),
                span: anchor..anchor,
            });
        }
    }

    Parsed {
        sections,
        junk: junk_spans,
        header_offsets,
        structure_errors,
    }
}

/// Full parser entry point: sections, junk lines, header offsets and the
/// structural (duplicate / missing section) errors.
pub fn parse_espf<'a>(input: &'a str) -> Parsed<'a> {
    parse_impl(input, input)
}

/// Compatibility entry point returning `(sections, junk, header_offsets)`.
///
/// Does NOT report duplicate or missing sections; use [`parse_espf`] and its
/// `structure_errors` for that.
pub fn parse_espf_runtime_with_diagnostics<'a>(
    input: &'a str,
) -> (
    HashMap<String, Vec<ParsedNode<'a>>>,
    Vec<JunkSpan>,
    HashMap<String, usize>,
) {
    let p = parse_espf(input);
    (p.sections, p.junk, p.header_offsets)
}

/// Backwards-compatible entry point: skips junk lines silently.
pub fn parse_espf_runtime<'a>(input: &'a str) -> HashMap<String, Vec<ParsedNode<'a>>> {
    parse_espf(input).sections
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parses the sample layout that used to be embedded in the CLI binary.
    /// All three sections must land with the expected node counts and ids.
    #[test]
    fn parses_sample_runtime_file() {
        let input = r#"
==============
= ESPF runtime file format instance
==============

# peripherals
  <gpio id="gpio4" pin="4" />
  <i2c  id="i2c0" sda="8" scl="9" frequency="400kHz" />
  <spi  id="spi2" sclk="6" mosi="7" />

# components
  <i2c_component  id="temp_bus"    i2c="$i2c0" address="0x48" />
  <spi_component  id="display_bus" spi="$spi2" mode="0" cs="$gpio5" frequency="40MHz" />

# devices
  <tmp102         id="outdoor_temp" i2c_component="$temp_bus" />
  <ili9341        id="tft_display"  spi_component="$display_bus" />
    "#;

        let sections = parse_espf_runtime(input);

        assert_eq!(sections["peripherals"].len(), 3);
        assert_eq!(sections["components"].len(), 2);
        assert_eq!(sections["devices"].len(), 2);

        let tmp102 = sections["devices"][0].clone();
        assert_eq!(tmp102.tag_name, "tmp102");
        assert_eq!(tmp102.id, "outdoor_temp");
    }

    /// A comment block may span multiple lines before and inside sections,
    /// and its body must not leak into sections as junk or nodes.
    #[test]
    fn block_comments_are_consumed_and_not_junk() {
        let input = concat!(
            "=\n",
            "title text survives on the opening line\n",
            "this body line and the blank one below are swallowed too\n",
            "\n",
            "==\n",
            "# peripherals\n",
            "  <gpio id=\"gpio4\" pin=\"4\" />\n",
            "=== trailing note\n",
            "closing line comes late\n",
            "=\n",
        );

        let (sections, junk, _) = parse_espf_runtime_with_diagnostics(input);

        assert!(junk.is_empty(), "comment lines must not be junk: {junk:?}");
        assert_eq!(sections["peripherals"].len(), 1);
    }

    /// A double-quoted value may not span lines: an unterminated quote must
    /// make the whole node line junk instead of reaching the next line's `"`.
    #[test]
    fn quoted_values_cannot_be_multiline() {
        let input = "# peripherals\n  <gpio id=\"gpio4\" pin=\"4 />\n  <spi id=\"spi2\" sclk=\"6\" mosi=\"7\" />\n";

        let (sections, junk, _) = parse_espf_runtime_with_diagnostics(input);

        let peripherals = &sections["peripherals"];
        assert_eq!(peripherals.len(), 1, "only the well-formed spi node parses");
        assert_eq!(peripherals[0].tag_name, "spi");

        assert_eq!(junk.len(), 1, "expected exactly one junk line");
        assert_eq!(junk[0].line, 2);
        assert_eq!(&input[junk[0].offset..junk[0].offset + 5], "<gpio");
    }

    /// Trailing text after a leaf node that is not an inline `//` comment
    /// is junk: the whole line is reported.
    #[test]
    fn non_comment_text_after_leaf_node_is_junk() {
        let input = "# peripherals\n  <spi id=\"spi2\" sclk=\"6\" mosi=\"7\" /> --- note\n";

        let (sections, junk, _) = parse_espf_runtime_with_diagnostics(input);

        assert!(sections["peripherals"].is_empty());
        assert_eq!(junk.len(), 1);
        assert_eq!(junk[0].line, 2);
    }

    /// An inline `//` comment directly after a self-closing element is
    /// grammar: the node parses and the comment is swallowed.
    #[test]
    fn inline_comment_after_leaf_node_parses() {
        let input = "# peripherals\n  <spi id=\"spi2\" sclk=\"6\" mosi=\"7\" /> // displays generally omit miso\n  <gpio id=\"gpio4\" pin=\"4\" />\n";

        let (sections, junk, _) = parse_espf_runtime_with_diagnostics(input);

        assert!(junk.is_empty(), "expected no junk, got {junk:?}");
        let peripherals = &sections["peripherals"];
        assert_eq!(peripherals.len(), 2);
        assert_eq!(peripherals[0].tag_name, "spi");
        assert_eq!(peripherals[0].get_attr("mosi"), Some("7"));
        assert_eq!(peripherals[1].tag_name, "gpio");
    }

    /// An inline `//` comment on its own line (not at the end of a
    /// self-closing element) is not grammar: it is reported as junk.
    #[test]
    fn inline_comment_not_after_leaf_node_is_junk() {
        let input = "# peripherals\n  // displays generally omit miso\n  <spi id=\"spi2\" sclk=\"6\" mosi=\"7\" />\n";

        let (sections, junk, _) = parse_espf_runtime_with_diagnostics(input);

        assert_eq!(junk.len(), 1, "standalone // line must be junk: {junk:?}");
        assert_eq!(junk[0].line, 2);
        // The well-formed node still parses.
        assert_eq!(sections["peripherals"].len(), 1);
        assert_eq!(sections["peripherals"][0].tag_name, "spi");
    }

    /// Inline trailing comments after a section heading are no longer
    /// grammar: the trailing text is reported as junk, but the heading and
    /// the rest of the section still parse.
    #[test]
    fn no_comment_after_section_heading() {
        let input = "# peripherals --- note\n  <spi id=\"spi2\" sclk=\"6\" mosi=\"7\" />\n";

        let (sections, junk, _) = parse_espf_runtime_with_diagnostics(input);

        assert_eq!(junk.len(), 1);
        assert_eq!(junk[0].line, 1);
        // The heading itself parsed, so the section is entered and its
        // well-formed nodes load.
        assert_eq!(sections["peripherals"].len(), 1);
        assert_eq!(sections["peripherals"][0].tag_name, "spi");
    }

    /// A malformed attribute value with no closing `"` must be reported as a
    /// junk line at a precise offset, not silently swallowed.
    #[test]
    fn rejects_node_with_missing_closing_quote() {
        let input = "# peripherals\n  <gpio id=\"gpio4\" pin=\"4 />\n";

        let (sections, junk, _) = parse_espf_runtime_with_diagnostics(input);

        // The malformed node must not come back with a truncated attribute.
        let peripherals = &sections["peripherals"];
        if !peripherals.is_empty() {
            let node = &peripherals[0];
            assert!(
                node.get_attr("pin").is_none(),
                "malformed pin attribute must not yield a value, got {:?}",
                node.get_attr("pin")
            );
        }

        // ...and the junk offset must point right at the node.
        assert_eq!(junk.len(), 1, "expected exactly one junk line");
        assert_eq!(junk[0].line, 2);
        assert_eq!(&input[junk[0].offset..junk[0].offset + 5], "<gpio");
    }

    /// A junk line inside a section is reported at the correct file offset.
    #[test]
    fn junk_offset_points_at_offending_line() {
        let input = "# peripherals\n<gpio id=\"a\"/ />\n";
        let (_sections, junk, _) = parse_espf_runtime_with_diagnostics(input);
        assert_eq!(junk.len(), 1);
        assert_eq!(junk[0].offset, "# peripherals\n".len());
    }

    // ---- ESPF v2 syntax --------------------------------------------------

    /// Dot-segment keys parse as one key, with its offset recorded.
    #[test]
    fn dotted_keys_parse() {
        let input = "# services\n  <wifi id=\"w\" a.b.c=\"x\" network1.ssid=\"home\" />\n";
        let (sections, junk, _) = parse_espf_runtime_with_diagnostics(input);
        assert!(junk.is_empty(), "{junk:?}");
        let attrs = &sections["services"][0].attributes;
        assert_eq!(attrs.len(), 3);
        assert_eq!(attrs[1].key, "a.b.c");
        assert_eq!(attrs[1].val, "x");
        assert_eq!(attrs[1].key_offset, input.find("a.b.c").unwrap());
        assert_eq!(attrs[2].key, "network1.ssid");
        // Single-segment keys still work and keep their offsets.
        assert_eq!(attrs[0].key, "id");
        assert_eq!(attrs[0].key_offset, input.find("id=").unwrap());
    }

    /// A dangling or doubled dot is not a key.
    #[test]
    fn malformed_dotted_keys_are_junk() {
        for bad in ["a.=\"x\"", ".a=\"x\"", "a..b=\"x\""] {
            let input = format!("# services\n  <t {bad} />\n");
            let (sections, junk, _) = parse_espf_runtime_with_diagnostics(&input);
            assert_eq!(junk.len(), 1, "{bad} should be junk");
            assert!(sections["services"].is_empty());
        }
    }

    /// `=` is Set, `+=` is Append; offsets point at key and value.
    #[test]
    fn set_and_append_operators_parse() {
        let input = "# services\n  <ntp id=\"n\" servers+=\"a.org\" servers+=\"b.org\" />\n";
        let (sections, junk, _) = parse_espf_runtime_with_diagnostics(input);
        assert!(junk.is_empty(), "{junk:?}");
        let attrs = &sections["services"][0].attributes;
        assert_eq!(attrs[0].op, AttrOp::Set);
        assert_eq!(attrs[1].op, AttrOp::Append);
        assert_eq!(attrs[1].key, "servers");
        assert_eq!(attrs[1].val, "a.org");
        assert_eq!(&input[attrs[1].val_offset..][..5], "a.org");
        assert_eq!(attrs[2].op, AttrOp::Append);
        assert_eq!(attrs[2].val, "b.org");
        // The `id` lookup only honours `=`.
        assert_eq!(sections["services"][0].id, "n");
    }

    #[test]
    fn append_works_on_dotted_keys() {
        let input = "# services\n  <t a.b+=\"x\" />\n";
        let (sections, junk, _) = parse_espf_runtime_with_diagnostics(input);
        assert!(junk.is_empty());
        let a = &sections["services"][0].attributes[0];
        assert_eq!((a.key, a.op), ("a.b", AttrOp::Append));
    }

    const ALL_FIVE: &str = "# espforge\n# peripherals\n# components\n# devices\n# services\n";

    #[test]
    fn all_five_sections_present_is_clean() {
        let p = parse_espf(ALL_FIVE);
        assert!(p.structure_errors.is_empty(), "{:?}", p.structure_errors);
        assert!(p.junk.is_empty());
        assert!(
            p.sections["services"].is_empty(),
            "empty sections are valid"
        );
    }

    #[test]
    fn duplicate_section_is_located_and_first_wins() {
        let src = "# espforge\n# peripherals\n  <gpio id=\"a\" pin=\"1\" />\n# components\n# devices\n# services\n# peripherals\n  <gpio id=\"b\" pin=\"2\" />\n";
        let p = parse_espf(src);
        assert_eq!(p.structure_errors.len(), 1, "{:?}", p.structure_errors);
        let e = &p.structure_errors[0];
        assert!(e.title.contains("duplicate section"));
        assert_eq!(&src[e.span.clone()], "# peripherals");
        // The *second* header is the one underlined.
        assert_eq!(e.span.start, src.rfind("# peripherals").unwrap());
        assert!(e.label.contains("line 2"), "{}", e.label);
        // First definition wins.
        assert_eq!(p.sections["peripherals"][0].id, "a");
    }

    #[test]
    fn missing_sections_point_at_last_header() {
        let src = "# espforge\n# peripherals\n";
        let p = parse_espf(src);
        let titles: Vec<&str> = p
            .structure_errors
            .iter()
            .map(|e| e.title.as_str())
            .collect();
        assert_eq!(
            titles,
            [
                "missing section `# components`",
                "missing section `# devices`",
                "missing section `# services`"
            ]
        );
        let last_header = src.find("# peripherals").unwrap();
        assert!(
            p.structure_errors
                .iter()
                .all(|e| e.span.start == last_header)
        );
    }

    #[test]
    fn empty_file_reports_all_five_missing_at_top() {
        let p = parse_espf("");
        assert_eq!(p.structure_errors.len(), 5);
        assert!(p.structure_errors.iter().all(|e| e.span == (0..0)));
    }
}
