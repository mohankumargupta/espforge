//! A snapshot corpus of rendered diagnostics: one bad spec per file under
//! `tests/diagnostics/specs/`, one expected transcript under
//! `tests/diagnostics/expected/`.
//!
//! This is the Phase 0 contract freeze from `extreme.md` §7: the refactor may
//! change *how* diagnostics are produced, but every message, title and primary
//! span a user sees today must survive until it is changed deliberately. Cases
//! whose file name ends in `silently-accepted` pin behaviour that no one
//! decided on — they are here so that changing it is a visible diff, not a
//! surprise.
//!
//! Regenerate after a deliberate change:
//!
//! ```text
//! UPDATE_DIAGNOSTICS=1 cargo test -p espforge --test diagnostics
//! ```
//!
//! Note what today's output shows and Phase 2 will deliberately change:
//! validation-stage problems exit 1, but problems found by the hardware stage
//! ("valid, but `build` cannot generate it yet") render as `error:` snippets
//! *and* still print `✓ ... is valid` with exit 0.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const BIN: &str = env!("CARGO_BIN_EXE_espforge");

fn suite_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("diagnostics")
}

/// Sorted `*.espf.txt` file names of the spec corpus.
fn spec_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .expect("read the diagnostics spec dir")
        .map(|entry| entry.expect("read spec dir entry").file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".espf.txt"))
        .collect();
    names.sort();
    names
}

/// Run `espforge check` on one spec, in a scratch dir, and render its transcript.
fn transcript(spec_dir: &Path, name: &str) -> String {
    let tmp = tempfile::tempdir().expect("create scratch dir");
    let body = fs::read(spec_dir.join(name)).expect("read spec");
    fs::write(tmp.path().join(name), body).expect("write spec");

    let out = Command::new(BIN)
        .args(["check", name])
        .current_dir(tmp.path())
        .env("TERM", "dumb")
        .env_remove("NO_COLOR")
        .env_remove("CLICOLOR")
        .env_remove("CLICOLOR_FORCE")
        .stdin(Stdio::null())
        .output()
        .expect("run espforge check");

    let code = match out.status.code() {
        Some(code) => code.to_string(),
        None => format!("signal {:?}", out.status),
    };

    let mut text = format!("$ espforge check {name}\nexit: {code}\n");
    text.push_str("--- stdout ---\n");
    text.push_str(&String::from_utf8_lossy(&out.stdout));
    text.push_str("--- stderr ---\n");
    text.push_str(&String::from_utf8_lossy(&out.stderr));

    if let Some(dir) = tmp.path().to_str() {
        text = text.replace(dir, "<cwd>");
    }
    text
}

#[test]
fn diagnostics_corpus_is_unchanged() {
    let suite = suite_dir();
    let spec_dir = suite.join("specs");
    let expected_dir = suite.join("expected");
    let update = std::env::var_os("UPDATE_DIAGNOSTICS").is_some_and(|v| v == "1");
    let names = spec_names(&spec_dir);
    assert!(!names.is_empty(), "no specs in {}", spec_dir.display());

    let mut mismatches = Vec::new();
    for name in &names {
        let actual = transcript(&spec_dir, name);
        let case = name.trim_end_matches(".espf.txt");
        let file = expected_dir.join(format!("{case}.txt"));

        if update {
            fs::create_dir_all(&expected_dir).expect("create expected dir");
            fs::write(&file, &actual).expect("write snapshot");
            continue;
        }

        match fs::read_to_string(&file) {
            Ok(expected) if expected == actual => {}
            Ok(expected) => mismatches.push(format!(
                "{case}: diagnostics changed\n--- expected ---\n{expected}--- actual ---\n{actual}"
            )),
            Err(_) => mismatches.push(format!("{case}: no snapshot at {}", file.display())),
        }
    }

    // A snapshot without a spec is dead weight: the corpus would keep passing
    // while the case it documents silently disappeared.
    if let Ok(entries) = fs::read_dir(&expected_dir) {
        for entry in entries.flatten() {
            let file = entry.path();
            let case = file
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let spec = format!("{}.espf.txt", case.trim_end_matches(".txt"));
            if !names.contains(&spec) {
                mismatches.push(format!("{case}: snapshot without a spec ({spec})"));
            }
        }
    }

    if !mismatches.is_empty() {
        panic!(
            "{} diagnostic case(s) differ. If the change is intended, run \
             `UPDATE_DIAGNOSTICS=1 cargo test -p espforge --test diagnostics` and \
             review the diff.\n\n{}",
            mismatches.len(),
            mismatches.join("\n")
        );
    }
}

/// The corpus must exercise every stage, not just the parser. If a refactor
/// moves a check into another layer, these cases keep pointing at it.
#[test]
fn corpus_covers_each_diagnostic_layer() {
    let names = spec_names(&suite_dir().join("specs"));
    for prefix in [
        "syntax-",
        "structure-",
        "chip-",
        "runtime-",
        "option-",
        "ref-",
        "gpio-",
        "i2c-",
        "uart-",
        "component-",
        "conflict-",
        "device-",
    ] {
        assert!(
            names.iter().any(|n| n.starts_with(prefix)),
            "no `{prefix}*` case in the diagnostics corpus"
        );
    }
}
