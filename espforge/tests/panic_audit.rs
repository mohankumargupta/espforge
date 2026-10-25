//! Adversarial specs must never panic the pipeline: every case ends in a
//! diagnostic, a clean refusal, or a board.
//!
//! This is Phase 2's panic audit (increment 6). The old panics it replaces —
//! the 128-slot peripheral table, `at_span(0..0)` in the conflict pass and
//! `parse_u8_attr`'s `unreachable!` — are gone with the code that held them.
//! These specs keep the replacement honest: the CLI must exit 0 or 1, write
//! valid JSON, and report a span inside the source even when the input is
//! empty, absurdly large, or incomplete.

use std::fs;
use std::process::{Command, Stdio};

use serde_json::Value;

const BIN: &str = env!("CARGO_BIN_EXE_espforge");

fn header() -> &'static str {
    "# espforge\n  <project name=\"adversarial\" />\n  <chip type=\"esp32c3\" />\n  <runtime type=\"blocking\" />\n\n# peripherals\n"
}

fn footer() -> &'static str {
    "\n# components\n\n# devices\n\n# services\n"
}

fn spec(peripherals: &str) -> String {
    format!("{}{peripherals}{}", header(), footer())
}

fn spec_with_components(peripherals: &str, components: &str) -> String {
    format!(
        "{}{peripherals}\n# components\n{components}\n# devices\n\n# services\n",
        header()
    )
}

/// What `check --format json` does with one spec, with the panic checks
/// already applied.
fn check(name: &str, source: &str) -> (Option<i32>, Value) {
    let tmp = tempfile::tempdir().expect("scratch dir");
    fs::write(tmp.path().join(name), source).expect("write adversarial spec");

    let out = Command::new(BIN)
        .args(["check", "--format", "json", name])
        .current_dir(tmp.path())
        .env("TERM", "dumb")
        .stdin(Stdio::null())
        .output()
        .expect("run espforge check --format json");

    assert!(
        out.status.code().is_some(),
        "{name} died from a signal: {:?}",
        out.status
    );
    assert!(
        out.stderr.is_empty(),
        "{name} wrote to stderr in JSON mode: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8(out.stdout).expect("JSON is UTF-8");
    let document: Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("{name}: stdout is not JSON: {e}\n{stdout}"));

    let diagnostics = document["diagnostics"]
        .as_array()
        .unwrap_or_else(|| panic!("{name}: no diagnostics array"));
    for diagnostic in diagnostics {
        let code = diagnostic["code"]
            .as_str()
            .unwrap_or_else(|| panic!("{name}: diagnostic without a code"));
        assert_ne!(code, "E0999", "{name}: unclassified diagnostic");
        assert!(
            !diagnostic["message"].as_str().unwrap_or("").is_empty(),
            "{name}: empty message"
        );
        assert_span(name, &diagnostic["primary"]["span"], source.len());
        for label in diagnostic["labels"].as_array().expect("labels array") {
            assert_span(name, &label["span"], source.len());
        }
    }

    if diagnostics.is_empty() {
        assert!(document["board"].is_object(), "{name}: no board");
    } else {
        assert!(document["board"].is_null(), "{name}: board on failure");
    }
    (out.status.code(), document)
}

fn assert_span(name: &str, span: &Value, source_len: usize) {
    let span = span
        .as_array()
        .unwrap_or_else(|| panic!("{name}: span is not an array"));
    assert_eq!(span.len(), 2, "{name}: span is not [start, end]");
    let start = span[0].as_u64().expect("span start") as usize;
    let end = span[1].as_u64().expect("span end") as usize;
    assert!(start <= end, "{name}: backwards span {start}..{end}");
    assert!(
        end <= source_len,
        "{name}: span {start}..{end} outside {source_len}"
    );
}

fn assert_clean_outcome(name: &str, source: &str) {
    let (status, _) = check(name, source);
    assert!(
        matches!(status, Some(0) | Some(1)),
        "{name}: unexpected exit status {status:?}"
    );
}

#[test]
fn a_129_peripheral_spec_is_not_special() {
    // The number the old fixed 128-slot table overflowed on.
    let mut peripherals = String::new();
    for pin in 0..129 {
        peripherals.push_str(&format!("  <gpio id=\"g{pin}\" pin=\"{pin}\" />\n"));
    }
    let source = spec(&peripherals);
    let (status, document) = check("many-peripherals.espf.txt", &source);
    assert_eq!(status, Some(0));
    assert!(document["diagnostics"].as_array().unwrap().is_empty());
    // Unused peripherals claim no pins; the point is that the 129th one is
    // accepted instead of overflowing a fixed table.
    assert!(document["board"].is_object());
    assert_eq!(
        document["board"]["resources"]["pins"]
            .as_array()
            .unwrap()
            .len(),
        0
    );
}

#[test]
fn a_129_component_spec_is_not_special() {
    let mut peripherals = String::new();
    let mut components = String::new();
    for pin in 0..129 {
        peripherals.push_str(&format!("  <gpio id=\"g{pin}\" pin=\"{pin}\" />\n"));
        components.push_str(&format!("  <led id=\"l{pin}\" gpio=\"$g{pin}\" />\n"));
    }
    let source = spec_with_components(&peripherals, &components);
    let (status, document) = check("many-components.espf.txt", &source);
    assert_eq!(status, Some(0));
    assert!(document["diagnostics"].as_array().unwrap().is_empty());
    assert_eq!(
        document["board"]["components"].as_array().unwrap().len(),
        129
    );
}

#[test]
fn numbers_too_large_for_the_target_type_are_diagnostics() {
    let source = spec(
        "  <gpio id=\"negative\" pin=\"-1\" />\n  \
           <gpio id=\"overflow\" pin=\"99999999999999999999\" />\n  \
           <i2c id=\"i2c0\" sda=\"4\" scl=\"5\" frequency=\"999999999999999999999kHz\" />\n  \
           <uart id=\"uart0\" tx=\"6\" rx=\"7\" />\n",
    );
    assert_clean_outcome("huge-numbers.espf.txt", &source);
}

#[test]
fn malformed_and_missing_attributes_are_diagnostics() {
    for (name, peripherals) in [
        ("empty-pin.espf.txt", "  <gpio id=\"g\" pin=\"\" />\n"),
        ("space-pin.espf.txt", "  <gpio id=\"g\" pin=\" 4\" />\n"),
        ("missing-pin.espf.txt", "  <gpio id=\"g\" />\n"),
        ("empty-id.espf.txt", "  <gpio id=\"\" pin=\"3\" />\n"),
        ("missing-id.espf.txt", "  <gpio pin=\"3\" />\n"),
        ("unsupported-pwm.espf.txt", "  <pwm id=\"p\" pin=\"4\" />\n"),
    ] {
        assert_clean_outcome(name, &spec(peripherals));
    }
}

#[test]
fn references_with_no_targets_are_diagnostics() {
    for (name, component) in [
        ("ref-bare.espf.txt", "<led id=\"l\" gpio=\"$\" />"),
        ("ref-dotted.espf.txt", "<led id=\"l\" gpio=\"$g.extra\" />"),
        ("ref-spaced.espf.txt", "<led id=\"l\" gpio=\"$g \" />"),
        ("ref-unknown.espf.txt", "<led id=\"l\" gpio=\"$nope\" />"),
    ] {
        let source = spec_with_components("  <gpio id=\"g\" pin=\"3\" />\n", component);
        assert_clean_outcome(name, &source);
    }
}

#[test]
fn truncated_and_duplicated_files_are_diagnostics() {
    assert_clean_outcome("empty.espf.txt", "");
    assert_clean_outcome("header-only.espf.txt", "# espforge\n");
    assert_clean_outcome(
        "duplicate-sections.espf.txt",
        "# espforge\n# peripherals\n# peripherals\n# components\n# devices\n# services\n",
    );
    assert_clean_outcome(
        "crlf.espf.txt",
        "# espforge\r\n  <project name=\"t\" />\r\n  <chip type=\"esp32c3\" />\r\n  <runtime type=\"blocking\" />\r\n\r\n# peripherals\r\n  <gpio id=\"g\" pin=\"3\" />\r\n\r\n# components\r\n\r\n# devices\r\n\r\n# services\r\n",
    );
}

#[test]
fn a_huge_line_does_not_panic() {
    let padding = "x".repeat(1024 * 1024);
    let source = format!("========\n{padding}\n========\n{}", spec(""));
    assert_clean_outcome("huge-line.espf.txt", &source);
}

#[test]
fn human_mode_never_prints_a_panic() {
    // `check`'s human mode is the path users see; a panic there would print
    // `panicked at` to stderr even when JSON mode stays clean.
    let tmp = tempfile::tempdir().expect("scratch dir");
    let name = "human-mode.espf.txt";
    fs::write(
        tmp.path().join(name),
        spec("  <gpio id=\"g\" pin=\"99999999999999999999\" />\n"),
    )
    .expect("write spec");

    let out = Command::new(BIN)
        .args(["check", name])
        .current_dir(tmp.path())
        .env("TERM", "dumb")
        .stdin(Stdio::null())
        .output()
        .expect("run espforge check");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "stderr: {stderr}");
    assert!(
        !stderr.contains("panicked at"),
        "human output panicked:\n{stderr}"
    );
    assert!(
        !String::from_utf8_lossy(&out.stdout).contains("is valid"),
        "a failing spec was reported valid"
    );
}
