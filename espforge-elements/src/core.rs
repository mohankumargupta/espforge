//! The `# espforge` section reader.
//!
//! It reads attributes by hand, exactly like the legacy reader it replaces:
//! the generated `Attrs` types have one consumer in this slice, so the reader
//! (and its tests and messages) is ported rather than rederived. The generator
//! arrives with `gpio`/`led`, where it has more than one consumer.
//!
//! Collects every problem instead of stopping at the first. Errors carry
//! byte-offset spans; `section_offset` is the byte offset of the `# espforge`
//! header and anchors errors about elements that are missing entirely.
//!
//! Returns the core settings (`Meta`) plus the `<options>` element if one was
//! given; the options themselves are interpreted by `Answers::from_node`.
//! Console-vs-chip rules live in `resolve`, not here.

use crate::chips;
use crate::model::{Console, Meta, Runtime};
use espf::context::ParsedNode;
use espf::diag::{attr_span, node_span};
use espforge_diagnostics::{Diagnostic, codes};

/// Read the `# espforge` section into [`Meta`] plus its `<options>` node.
pub fn core_from_section<'n, 'a>(
    section_offset: usize,
    nodes: &'n [ParsedNode<'a>],
) -> Result<(Meta, Option<&'n ParsedNode<'a>>), Vec<Diagnostic>> {
    let mut errors = Vec::new();
    let mut project: Option<&ParsedNode> = None;
    let mut chip_node: Option<&ParsedNode> = None;
    let mut runtime: Option<&ParsedNode> = None;
    let mut console: Option<&ParsedNode> = None;
    let mut options: Option<&'n ParsedNode<'a>> = None;

    for n in nodes {
        match n.tag_name {
            "project" if project.is_none() => project = Some(n),
            "chip" if chip_node.is_none() => chip_node = Some(n),
            "runtime" if runtime.is_none() => runtime = Some(n),
            "console" if console.is_none() => console = Some(n),
            "options" if options.is_none() => options = Some(n),
            "project" | "chip" | "runtime" | "console" | "options" => {
                errors.push(Diagnostic::error(
                    codes::CORE_DUPLICATE_ELEMENT,
                    node_span(n),
                    format!("duplicate <{}> in # espforge", n.tag_name),
                    "keep only one of each element in `# espforge`",
                ))
            }
            other => errors.push(Diagnostic::error(
                codes::CORE_UNKNOWN_ELEMENT,
                node_span(n),
                format!("unknown element <{other}> in # espforge"),
                "expected one of <project>, <chip>, <runtime>, <console>, <options>",
            )),
        }
    }

    let short = |label: &str| {
        Diagnostic::error(
            codes::CORE_INCOMPLETE,
            section_offset..section_offset,
            "# espforge is incomplete",
            label,
        )
    };

    let name = project.and_then(|p| p.get_attr("name"));
    if name.is_none() {
        errors.push(short("# espforge needs <project name=\"...\" />"));
    }

    // Chip: present, and a chip esp-metadata knows about.
    let chip = match chip_node.map(|n| (n, n.get_attr("type"))) {
        None => {
            errors.push(short("# espforge needs <chip type=\"...\" />"));
            None
        }
        Some((node, None)) => {
            errors.push(Diagnostic::error(
                codes::CORE_CHIP_WITHOUT_TYPE,
                node_span(node),
                "<chip> is missing its type",
                "expected <chip type=\"esp32c3\" />",
            ));
            None
        }
        Some((node, Some(c))) => {
            if chips::is_supported_chip(c) {
                Some(c)
            } else {
                let attr = node.get_attr_spanned("type").expect("type attr present");
                errors.push(Diagnostic::error(
                    codes::CORE_UNKNOWN_CHIP,
                    attr_span(attr),
                    "unknown chip",
                    format!(
                        "\"{c}\" is not a supported chip (expected one of: {})",
                        chips::supported_chips().join(", ")
                    ),
                ));
                None
            }
        }
    };

    let runtime = match runtime.map(|n| n.get_attr("type")) {
        Some(Some("blocking")) => Some(Runtime::Blocking),
        Some(Some("embassy")) => Some(Runtime::Embassy),
        Some(Some(other)) => {
            let node = runtime.unwrap();
            let attr = node.get_attr_spanned("type").expect("type attr present");
            errors.push(Diagnostic::error(
                codes::CORE_UNKNOWN_RUNTIME,
                attr_span(attr),
                "unknown runtime",
                format!("\"{other}\" is not a runtime (expected \"blocking\" or \"embassy\")"),
            ));
            None
        }
        Some(None) => {
            errors.push(Diagnostic::error(
                codes::CORE_RUNTIME_WITHOUT_TYPE,
                node_span(runtime.unwrap()),
                "<runtime> is missing its type",
                "expected <runtime type=\"blocking\" /> or <runtime type=\"embassy\" />",
            ));
            None
        }
        None => {
            errors.push(short("# espforge needs <runtime type=\"...\" />"));
            None
        }
    };

    // Optional; defaults to UART.
    let console = match console.map(|n| (n, n.get_attr("type"))) {
        None => Console::default(),
        Some((_, Some("uart"))) => Console::Uart,
        Some((_, Some("jtag-serial"))) => Console::JtagSerial,
        Some((node, Some(other))) => {
            let attr = node.get_attr_spanned("type").expect("type attr present");
            errors.push(Diagnostic::error(
                codes::CORE_UNKNOWN_CONSOLE,
                attr_span(attr),
                "unknown console",
                format!("\"{other}\" is not a console (expected \"uart\" or \"jtag-serial\")"),
            ));
            Console::default()
        }
        Some((node, None)) => {
            errors.push(Diagnostic::error(
                codes::CORE_CONSOLE_WITHOUT_TYPE,
                node_span(node),
                "<console> is missing its type",
                "expected <console type=\"uart\" /> or <console type=\"jtag-serial\" />",
            ));
            Console::default()
        }
    };

    match (name, chip, runtime) {
        (Some(name), Some(chip), Some(runtime)) if errors.is_empty() => Ok((
            Meta {
                name: name.to_string(),
                description: project
                    .and_then(|p| p.get_attr("description"))
                    .map(str::to_string),
                chip: chip.to_string(),
                runtime,
                console,
            },
            options,
        )),
        _ => Err(errors),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use espf::parser::parse_espf_runtime_with_diagnostics;

    const HEAD: &str = "# espforge\n  <project name=\"hello\"/>\n  <runtime type=\"blocking\" />\n";

    fn core(rest: &str) -> Result<Meta, Vec<Diagnostic>> {
        let src = format!("{HEAD}{rest}");
        let (sections, _junk, offsets) = parse_espf_runtime_with_diagnostics(&src);
        core_from_section(offsets["espforge"], &sections["espforge"]).map(|(m, _)| m)
    }

    #[test]
    fn parses_helloworld_header() {
        let m = core("  <chip type=\"esp32c3\" />\n").unwrap();
        assert_eq!(m.chip, "esp32c3");
        assert_eq!(m.runtime, Runtime::Blocking);
    }

    #[test]
    fn console_defaults_to_uart_and_can_be_jtag() {
        assert_eq!(
            core("  <chip type=\"esp32c3\" />\n").unwrap().console,
            Console::Uart
        );
        let m = core("  <chip type=\"esp32c3\" />\n  <console type=\"jtag-serial\" />\n").unwrap();
        assert_eq!(m.console, Console::JtagSerial);
    }

    #[test]
    fn unknown_console_points_at_the_value() {
        let src = format!("{HEAD}  <chip type=\"esp32c3\" />\n  <console type=\"usb\" />\n");
        let (sections, _, offsets) = parse_espf_runtime_with_diagnostics(&src);
        let errs = core_from_section(offsets["espforge"], &sections["espforge"]).unwrap_err();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, codes::CORE_UNKNOWN_CONSOLE);
        assert_eq!(&src[errs[0].primary.clone()], "usb");
    }

    #[test]
    fn unknown_chip_is_reported_at_the_value() {
        let src = format!("{HEAD}  <chip type=\"esp99\" />\n");
        let (sections, _, offsets) = parse_espf_runtime_with_diagnostics(&src);
        let errs = core_from_section(offsets["espforge"], &sections["espforge"]).unwrap_err();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].code, codes::CORE_UNKNOWN_CHIP);
        assert_eq!(errs[0].message, "unknown chip");
        assert_eq!(&src[errs[0].primary.clone()], "esp99");
    }

    #[test]
    fn unknown_runtime_points_at_the_value() {
        let src = "# espforge\n  <project name=\"h\" />\n  <chip type=\"esp32c3\" />\n  <runtime type=\"fome\" />\n";
        let (sections, _, offsets) = parse_espf_runtime_with_diagnostics(src);
        let errs = core_from_section(offsets["espforge"], &sections["espforge"]).unwrap_err();
        assert_eq!(errs[0].code, codes::CORE_UNKNOWN_RUNTIME);
        assert_eq!(&src[errs[0].primary.clone()], "fome");
    }

    #[test]
    fn options_element_is_returned() {
        let src = format!("{HEAD}  <chip type=\"esp32c3\" />\n  <options defaults=\"true\" />\n");
        let (sections, _, offsets) = parse_espf_runtime_with_diagnostics(&src);
        let (_, opts) = core_from_section(offsets["espforge"], &sections["espforge"]).unwrap();
        assert_eq!(opts.unwrap().tag_name, "options");
    }

    #[test]
    fn missing_chip_is_reported_at_the_section_header() {
        let src = HEAD.to_string();
        let (sections, _, offsets) = parse_espf_runtime_with_diagnostics(&src);
        let errs = core_from_section(offsets["espforge"], &sections["espforge"]).unwrap_err();
        assert!(errs.iter().any(|e| e.labels[0].text.contains("<chip")));
        assert!(errs.iter().all(|e| e.code == codes::CORE_INCOMPLETE));
        assert!(errs.iter().all(|e| src[e.primary.start..].starts_with('#')));
    }

    #[test]
    fn unknown_and_duplicate_elements_are_located() {
        let src =
            format!("{HEAD}  <chip type=\"esp32c3\" />\n  <gizmo />\n  <chip type=\"esp32\" />\n");
        let (sections, _, offsets) = parse_espf_runtime_with_diagnostics(&src);
        let errs = core_from_section(offsets["espforge"], &sections["espforge"]).unwrap_err();
        assert_eq!(errs.len(), 2);
        assert_eq!(errs[0].code, codes::CORE_UNKNOWN_ELEMENT);
        assert_eq!(&src[errs[0].primary.clone()], "<gizmo");
        assert_eq!(errs[1].code, codes::CORE_DUPLICATE_ELEMENT);
        assert!(errs[1].message.contains("duplicate <chip>"));
    }
}
