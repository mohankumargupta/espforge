//! The `# espforge` section reader (replaces the old `parse_meta`).

use crate::chips;
use espf::context::ParsedNode;
use espf::diag::EspfError;
use espforge_model::{Console, Meta, Runtime};

/// Fixed-schema reader for the `# espforge` section.
///
/// Collects every problem instead of stopping at the first. Errors carry
/// byte-offset spans; `section_offset` is the byte offset of the `# espforge`
/// header and anchors errors about elements that are missing entirely.
///
/// Returns the core settings (`Meta`) plus the `<options>` element if one was
/// given; the options themselves are interpreted by `Answers::from_node`.
/// Console-vs-chip rules live in `resolve`, not here.
pub fn core_from_section<'n, 'a>(
    section_offset: usize,
    nodes: &'n [ParsedNode<'a>],
) -> Result<(Meta, Option<&'n ParsedNode<'a>>), Vec<EspfError>> {
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
                errors.push(EspfError::at_node(
                    n,
                    format!("duplicate <{}> in # espforge", n.tag_name),
                    "keep only one of each element in `# espforge`",
                ))
            }
            other => errors.push(EspfError::at_node(
                n,
                format!("unknown element <{other}> in # espforge"),
                "expected one of <project>, <chip>, <runtime>, <console>, <options>",
            )),
        }
    }

    let short = |label: &str| EspfError {
        title: "# espforge is incomplete".into(),
        label: label.into(),
        span: section_offset..section_offset,
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
            errors.push(EspfError::at_node(
                node,
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
                errors.push(EspfError::at_attr(
                    attr,
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
            errors.push(EspfError::at_attr(
                attr,
                "unknown runtime",
                format!("\"{other}\" is not a runtime (expected \"blocking\" or \"embassy\")"),
            ));
            None
        }
        Some(None) => {
            errors.push(EspfError::at_node(
                runtime.unwrap(),
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
            errors.push(EspfError::at_attr(
                attr,
                "unknown console",
                format!("\"{other}\" is not a console (expected \"uart\" or \"jtag-serial\")"),
            ));
            Console::default()
        }
        Some((node, None)) => {
            errors.push(EspfError::at_node(
                node,
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

    fn core(rest: &str) -> Result<Meta, Vec<EspfError>> {
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
        assert_eq!(&src[errs[0].span.clone()], "usb");
    }

    #[test]
    fn unknown_chip_is_reported_at_the_value() {
        let src = format!("{HEAD}  <chip type=\"esp99\" />\n");
        let (sections, _, offsets) = parse_espf_runtime_with_diagnostics(&src);
        let errs = core_from_section(offsets["espforge"], &sections["espforge"]).unwrap_err();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].title, "unknown chip");
        assert_eq!(&src[errs[0].span.clone()], "esp99");
    }

    #[test]
    fn unknown_runtime_points_at_the_value() {
        let src = "# espforge\n  <project name=\"h\" />\n  <chip type=\"esp32c3\" />\n  <runtime type=\"fome\" />\n";
        let (sections, _, offsets) = parse_espf_runtime_with_diagnostics(src);
        let errs = core_from_section(offsets["espforge"], &sections["espforge"]).unwrap_err();
        assert_eq!(&src[errs[0].span.clone()], "fome");
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
        assert!(errs.iter().any(|e| e.label.contains("<chip")));
        assert!(errs.iter().all(|e| src[e.span.start..].starts_with('#')));
    }

    #[test]
    fn unknown_and_duplicate_elements_are_located() {
        let src =
            format!("{HEAD}  <chip type=\"esp32c3\" />\n  <gizmo />\n  <chip type=\"esp32\" />\n");
        let (sections, _, offsets) = parse_espf_runtime_with_diagnostics(&src);
        let errs = core_from_section(offsets["espforge"], &sections["espforge"]).unwrap_err();
        assert_eq!(errs.len(), 2);
        assert_eq!(&src[errs[0].span.clone()], "<gizmo");
        assert!(errs[1].title.contains("duplicate <chip>"));
    }
}
