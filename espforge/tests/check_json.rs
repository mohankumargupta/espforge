//! `check --format json` contract: one document per run, stable codes,
//! in-range spans, and a board exactly when the spec lowers.
//!
//! The corpus and every embedded example are enumerated, so a new case
//! cannot pass by being absent from a hand-written list.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;

const BIN: &str = env!("CARGO_BIN_EXE_espforge");

fn suite_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("diagnostics")
}

fn run_check_json(args: &[&str], cwd: &Path) -> (Option<i32>, Value) {
    let out = Command::new(BIN)
        .arg("check")
        .arg("--format")
        .arg("json")
        .args(args)
        .current_dir(cwd)
        .env("TERM", "dumb")
        .stdin(Stdio::null())
        .output()
        .expect("run espforge check --format json");

    assert!(
        out.stderr.is_empty(),
        "JSON mode writes stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).expect("JSON is UTF-8");
    let value: Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout is not JSON: {e}\n{stdout}"));

    (out.status.code(), value)
}

fn assert_code(code: &Value, message: &Value) {
    let code = code
        .as_str()
        .unwrap_or_else(|| panic!("missing code: {message}"));
    assert_eq!(code.len(), 5, "code {code} is not 5 chars");
    let (prefix, digits) = code.split_at(1);
    assert!(matches!(prefix, "E" | "W"), "code {code} has no E/W prefix");
    assert!(
        digits.chars().all(|c| c.is_ascii_digit()),
        "code {code} has non-digits"
    );
    assert_ne!(code, "E0999", "unclassified diagnostic: {message}");
}

fn assert_span(span: &Value, source_len: usize) {
    let span = span.as_array().expect("span is an array");
    assert_eq!(span.len(), 2, "span is [start, end]");
    let start = span[0].as_u64().expect("span start") as usize;
    let end = span[1].as_u64().expect("span end") as usize;
    assert!(start <= end, "span {start}..{end} is backwards");
    assert!(
        end <= source_len,
        "span {start}..{end} outside {source_len}"
    );
}

#[test]
fn every_diagnostics_spec_has_one_coded_document() {
    let spec_dir = suite_dir().join("specs");
    let tmp = tempfile::tempdir().expect("scratch dir");
    let mut names: Vec<String> = fs::read_dir(&spec_dir)
        .expect("read specs")
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".espf.txt"))
        .collect();
    names.sort();
    assert!(!names.is_empty());

    for name in names {
        let source = fs::read_to_string(spec_dir.join(&name)).expect("read spec");
        fs::write(tmp.path().join(&name), &source).expect("copy spec");

        let (status, document) = run_check_json(&[&name], tmp.path());
        assert_eq!(document["file"], name);
        assert!(document["notes"].is_array());

        let diagnostics = document["diagnostics"]
            .as_array()
            .expect("diagnostics array");
        for diagnostic in diagnostics {
            assert_code(&diagnostic["code"], &diagnostic["message"]);
            assert!(
                !diagnostic["message"].as_str().unwrap_or("").is_empty(),
                "empty message"
            );
            assert_span(&diagnostic["primary"]["span"], source.len());
            assert_eq!(diagnostic["primary"]["file"], name);
            for label in diagnostic["labels"].as_array().expect("labels array") {
                assert_span(&label["span"], source.len());
            }
        }

        if diagnostics.is_empty() {
            assert_eq!(status, Some(0), "{name} passed with no diagnostics");
            assert!(document["board"].is_object(), "{name}: no board");
        } else {
            assert_eq!(status, Some(1), "{name} had diagnostics but passed");
            assert!(document["board"].is_null(), "{name}: board on failure");
        }
    }
}

#[test]
fn every_example_gets_a_board_exactly_when_it_lowers() {
    for category in espforge_examples::categories() {
        for example in &category.examples {
            let path = format!("{}/{}", category.name, example.name);
            let (status, document) = run_check_json(&["--example", &path], Path::new("."));
            assert_eq!(document["file"], format!("embedded:{path}"));
            let diagnostics = document["diagnostics"]
                .as_array()
                .expect("diagnostics array");

            if diagnostics.is_empty() {
                assert_eq!(status, Some(0), "{path}: passed with no diagnostics");
                assert!(document["board"].is_object(), "{path}: no board");
            } else {
                assert_eq!(status, Some(1), "{path}: had diagnostics but passed");
                assert!(document["board"].is_null(), "{path}: board on failure");
                for diagnostic in diagnostics {
                    assert_code(&diagnostic["code"], &diagnostic["message"]);
                }
            }
        }
    }
}

#[test]
fn blink_board_json_has_the_documented_shape() {
    let (status, document) = run_check_json(&["--example", "01.Basics/blink"], Path::new("."));
    assert_eq!(status, Some(0));
    assert!(document["diagnostics"].as_array().unwrap().is_empty());

    let board = &document["board"];
    assert_eq!(board["meta"]["name"], "blink");
    assert_eq!(board["meta"]["chip"], "esp32c3");
    assert_eq!(board["meta"]["runtime"], "blocking");
    assert_eq!(board["meta"]["console"], "uart");

    let component = &board["components"][0];
    assert_eq!(component["kind"], "led");
    assert_eq!(component["id"]["id"], "red_led");
    assert_eq!(component["pin"], 18);
    assert_eq!(component["active_low"], false);
    assert!(component["id"]["span"].is_array());

    let claim = &board["resources"]["pins"][0];
    assert_eq!(claim["pin"], 18);
    assert_eq!(claim["use"], "output");
    assert_eq!(claim["owner"]["kind"], "led");
    assert_eq!(claim["owner"]["id"], "red_led");
    assert!(claim["owner"]["span"].is_array());
}

#[test]
fn a_hardware_error_is_not_reported_as_valid_in_json_or_text() {
    let tmp = tempfile::tempdir().unwrap();
    let file = tmp.path().join("pwm.espf");
    fs::write(
        &file,
        "# espforge\n  <project name=\"t\" />\n  <chip type=\"esp32c3\" />\n  <runtime type=\"blocking\" />\n# peripherals\n  <pwm id=\"pwm0\" />\n# components\n# devices\n# services\n",
    )
    .unwrap();

    let (status, document) = run_check_json(&["pwm.espf"], tmp.path());
    assert_eq!(status, Some(1));
    assert!(document["board"].is_null());
    assert_eq!(document["diagnostics"][0]["code"], "E0501");

    let text = Command::new(BIN)
        .args(["check", "pwm.espf"])
        .current_dir(tmp.path())
        .output()
        .expect("run espforge check");
    assert_eq!(text.status.code(), Some(1));
    assert!(
        !String::from_utf8_lossy(&text.stdout).contains("is valid"),
        "check still claims a failing spec is valid"
    );
}
