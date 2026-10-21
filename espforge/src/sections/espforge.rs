use espf::context::ParsedNode;
use espf::diag::EspfError;
use espforge_model::{Console, Meta, Runtime};

/// Fixed-schema builtin for the `# espforge` section.
/// Collects every problem instead of stopping at the first.
///
/// Errors carry byte-offset spans into the original file so the CLI can
/// render each one as an `annotate-snippets` block. `section_offset` is the
/// byte offset of the `# espforge` line and is used to locate errors about
/// elements that are missing entirely.
pub fn parse_meta(
    section_offset: usize,
    nodes: &[ParsedNode],
) -> Result<Meta, Vec<EspfError>> {
    let mut errors = Vec::new();
    let mut project: Option<&ParsedNode> = None;
    let mut chip: Option<&str> = None;
    let mut runtime: Option<&ParsedNode> = None;
    let mut console: Option<&ParsedNode> = None;

    for n in nodes {
        match n.tag_name {
            "project" if project.is_none() => project = Some(n),
            "chip" if chip.is_none() => chip = n.get_attr("type"),
            "runtime" if runtime.is_none() => runtime = Some(n),
            "console" if console.is_none() => console = Some(n),
            "project" | "chip" | "runtime" | "console" => {
                errors.push(EspfError::at_node(
                    n,
                    format!("duplicate <{}> in # espforge", n.tag_name),
                    "keep only one of each element in `# espforge`",
                ))
            }
            other => errors.push(EspfError::at_node(
                n,
                format!("unknown element <{other}> in # espforge"),
                "expected one of <project>, <chip>, <runtime>, <console>",
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
    if chip.is_none() {
        errors.push(short("# espforge needs <chip type=\"...\" />"));
    }
    let runtime = match runtime.map(|n| n.get_attr("type")) {
        Some(Some("blocking")) => Some(Runtime::Blocking),
        Some(Some("embassy")) => Some(Runtime::Embassy),
        Some(Some(other)) => {
            let node = runtime.unwrap();
            // `Some(Some(other))` guarantees the `type` attribute exists.
            let attr = node.get_attr_spanned("type").expect("type attr present");
            errors.push(EspfError {
                title: "unknown runtime".into(),
                label: format!("\"{other}\" is not a runtime (expected \"blocking\" or \"embassy\")"),
                span: attr.val_offset..attr.val_offset + attr.val.len(),
            });
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
    let console_node = console;
    let console = match console.map(|n| (n, n.get_attr("type"))) {
        None => Console::default(),
        Some((_, Some("uart"))) => Console::Uart,
        Some((_, Some("jtag-serial"))) => Console::JtagSerial,
        Some((node, Some(other))) => {
            let span = node
                .get_attr_spanned("type")
                .map_or_else(
                    || espf::diag::node_span(node),
                    |a| a.val_offset..a.val_offset + a.val.len(),
                );
            errors.push(EspfError {
                title: "unknown console".into(),
                label: format!(
                    "\"{other}\" is not a console (expected \"uart\" or \"jtag-serial\")"
                ),
                span,
            });
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
    // Only the case we are sure of; other UART-only chips can be added here later.
    if chip == Some("esp32") && console == Console::JtagSerial {
        let span = console_node
            .map(espf::diag::node_span)
            .unwrap_or_else(|| section_offset..section_offset);
        errors.push(EspfError {
            title: "esp32 has no USB-Serial-JTAG".into(),
            label: "esp32 boards have no native USB; use <console type=\"uart\" />".into(),
            span,
        });
    }

    match (name, chip, runtime) {
        (Some(name), Some(chip), Some(runtime)) if errors.is_empty() => Ok(Meta {
            name: name.to_string(),
            description: project
                .and_then(|p| p.get_attr("description"))
                .map(str::to_string),
            chip: chip.to_string(),
            runtime,
            console,
        }),
        _ => Err(errors),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEAD: &str = "# espforge\n  <project name=\"hello\"/>\n  <runtime type=\"blocking\" />\n";

    fn meta(chip_and_console: &str) -> Result<Meta, Vec<EspfError>> {
        let src = format!("{HEAD}{chip_and_console}");
        let (sections, _junk, section_offsets) =
            espf::parser::parse_espf_runtime_with_diagnostics(&src);
        parse_meta(
            section_offsets.get("espforge").copied().unwrap_or(0),
            &sections["espforge"],
        )
    }

    #[test]
    fn parses_helloworld_header() {
        let m = meta("  <chip type=\"esp32c3\" />\n").unwrap();
        assert_eq!(m.chip, "esp32c3");
        assert_eq!(m.runtime, Runtime::Blocking);
    }

    #[test]
    fn console_defaults_to_uart() {
        assert_eq!(
            meta("  <chip type=\"esp32c3\" />\n").unwrap().console,
            Console::Uart
        );
    }

    #[test]
    fn console_can_be_jtag_serial() {
        let m = meta("  <chip type=\"esp32c3\" />\n  <console type=\"jtag-serial\" />\n").unwrap();
        assert_eq!(m.console, Console::JtagSerial);
    }

    #[test]
    fn unknown_console_is_reported() {
        assert!(meta("  <chip type=\"esp32c3\" />\n  <console type=\"usb\" />\n").is_err());
    }

    #[test]
    fn esp32_rejects_jtag_serial() {
        assert!(meta("  <chip type=\"esp32\" />\n  <console type=\"jtag-serial\" />\n").is_err());
    }

    #[test]
    fn missing_chip_is_reported() {
        let (sections, _junk, section_offsets) =
            espf::parser::parse_espf_runtime_with_diagnostics(HEAD);
        assert!(parse_meta(
            section_offsets.get("espforge").copied().unwrap_or(0),
            &sections["espforge"]
        )
        .is_err());
    }

    /// Errors about unknown/duplicate elements point at the actual node.
    #[test]
    fn unknown_element_error_has_a_located_span() {
        let src = "# espforge\n  <project name=\"hello\"/>\n  <chip type=\"esp32c3\" />\n  <runtime type=\"blocking\" />\n  <gizmo />\n";
        let (sections, _junk, section_offsets) =
            espf::parser::parse_espf_runtime_with_diagnostics(src);
        let err = parse_meta(
            section_offsets.get("espforge").copied().unwrap_or(0),
            &sections["espforge"],
        )
        .unwrap_err();
        assert_eq!(err.len(), 1);
        assert_eq!(&src[err[0].span.start..err[0].span.end], "<gizmo");
        assert_eq!(err[0].span.start, src.find("<gizmo").unwrap());
    }

    /// Errors about an entirely missing element point back at the section
    /// header instead of nothing.
    #[test]
    fn missing_element_error_points_at_section_header() {
        let src = "# espforge\n  <chip type=\"esp32c3\" />\n";
        let (sections, _junk, section_offsets) =
            espf::parser::parse_espf_runtime_with_diagnostics(src);
        let err = parse_meta(
            section_offsets.get("espforge").copied().unwrap_or(0),
            &sections["espforge"],
        )
        .unwrap_err();
        assert!(err.iter().any(|e| e.label.contains("<project")));
        // All such errors point at the `# espforge` line.
        assert!(err.iter().all(|e| src[e.span.start..].starts_with('#')));
    }
}
