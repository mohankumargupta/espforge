//! Byte-exact CLI transcripts: stdout, stderr and exit code for every
//! non-interactive command.
//!
//! This is the Phase 0 contract freeze from `extreme.md` §6/§7: the CLI surface
//! (command names, flags, aliases, output, exit codes 0/1/2) is expected to
//! survive the refactor unchanged, so any byte that changes here must be a
//! deliberate, reviewed diff rather than an accident.
//!
//! Regenerate the snapshots with:
//!
//! ```text
//! UPDATE_TRANSCRIPTS=1 cargo test -p espforge --test transcripts
//! ```
//!
//! Then read the diff before committing it.
//!
//! Not snapshotted, because the output depends on the host or needs a TTY:
//! `doctor` (probes the local toolchain), `new` without `--yes` and bare
//! `examples` (both refuse to run without a terminal — covered structurally
//! below). `build` is only covered on its no-argument error path; the happy
//! path spawns cargo and belongs to the golden-project tests.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The binary under test, built by cargo for this integration test.
const BIN: &str = env!("CARGO_BIN_EXE_espforge");

/// A spec with one peripheral and one component, used by the `check` and `gen`
/// cases. Kept inline so the transcript does not depend on example drift.
const BLINK: &str = "\
# espforge
  <project name=\"blink\" />
  <chip type=\"esp32c3\" />
  <runtime type=\"blocking\" />

# peripherals
  <gpio id=\"gpio18\" pin=\"18\" direction=\"output\" />

# components
  <led id=\"red_led\" gpio=\"$gpio18\" />

# devices

# services
";

/// A spec missing three mandatory sections, for the diagnostics shape.
const MISSING_SECTIONS: &str = "\
# peripherals
  <gpio id=\"gpio18\" pin=\"18\" />

# devices
";

/// The app logic `gen` requires next to the spec, so the generation case is
/// the happy path rather than the missing-app error.
const APP: &str = "\
use crate::Context;

pub fn setup(ctx: &mut Context) {
    ctx.logger.info(\"blink example\");
}

pub fn forever(ctx: &mut Context) {
    let led = component!(ctx, red_led);
    led.toggle();
    ctx.delay.delay_ms(1000);
}
";

/// A spec with an unclosed attribute quote, for the syntax-error shape.
const SYNTAX_ERROR: &str = "\
# espforge
  <project name=\"bad\" />

# peripherals
  <gpio id=\"gpio4\" pin=\"4 />

# components

# devices

# services
";

struct Case {
    /// Snapshot file name, `<name>.txt`.
    name: &'static str,
    args: &'static [&'static str],
    /// Files written into the scratch directory before the command runs.
    files: &'static [(&'static str, &'static str)],
}

const CASES: &[Case] = &[
    // --- top level ---------------------------------------------------------
    Case {
        name: "no-command",
        args: &[],
        files: &[],
    },
    Case {
        name: "help",
        args: &["--help"],
        files: &[],
    },
    Case {
        name: "version",
        args: &["version"],
        files: &[],
    },
    // --- espf catalog ------------------------------------------------------
    Case {
        name: "espf",
        args: &["espf"],
        files: &[],
    },
    Case {
        name: "espf-help-view",
        args: &["espf", "help"],
        files: &[],
    },
    Case {
        name: "espf-overview",
        args: &["espf", "overview"],
        files: &[],
    },
    Case {
        name: "espf-peripherals",
        args: &["espf", "peripherals"],
        files: &[],
    },
    Case {
        name: "espf-components",
        args: &["espf", "components"],
        files: &[],
    },
    Case {
        name: "espf-led-man-page",
        args: &["espf", "components", "led", "help"],
        files: &[],
    },
    Case {
        name: "espf-led-examples",
        args: &["espf", "components", "led", "examples"],
        files: &[],
    },
    Case {
        name: "espf-gpio-man-page",
        args: &["espf", "peripherals", "gpio", "help"],
        files: &[],
    },
    Case {
        name: "espf-i2c-man-page",
        args: &["espf", "peripherals", "i2c", "help"],
        files: &[],
    },
    Case {
        name: "espf-i2c-examples",
        args: &["espf", "peripherals", "i2c", "examples"],
        files: &[],
    },
    Case {
        name: "espf-shorthand-element",
        args: &["espf", "i2c"],
        files: &[],
    },
    Case {
        name: "espf-unknown-element",
        args: &["espf", "nosuch"],
        files: &[],
    },
    // --- examples plumbing -------------------------------------------------
    Case {
        name: "examples-categories",
        args: &["examples", "categories"],
        files: &[],
    },
    Case {
        name: "examples-list",
        args: &["examples", "list"],
        files: &[],
    },
    Case {
        name: "examples-list-category",
        args: &["examples", "list", "01.Basics"],
        files: &[],
    },
    Case {
        name: "examples-list-unknown-category",
        args: &["examples", "list", "99.Nope"],
        files: &[],
    },
    Case {
        name: "examples-get",
        args: &["examples", "get", "01.Basics/helloworld", "--out", "hw"],
        files: &[],
    },
    Case {
        name: "examples-get-refuses-overwrite",
        args: &["examples", "get", "01.Basics/helloworld", "--out", "hw"],
        files: &[("hw/example.espf.txt", "already here\n")],
    },
    Case {
        name: "examples-get-unknown-example",
        args: &["examples", "get", "99.Nope/nope", "--out", "hw"],
        files: &[],
    },
    // --- check -------------------------------------------------------------
    Case {
        name: "check-example",
        args: &["check", "--example", "01.Basics/helloworld"],
        files: &[],
    },
    Case {
        name: "check-unknown-example",
        args: &["check", "--example", "99.Nope/nope"],
        files: &[],
    },
    Case {
        name: "check-valid-file",
        args: &["check", "blink.espf.txt"],
        files: &[("blink.espf.txt", BLINK)],
    },
    Case {
        name: "check-missing-sections",
        args: &["check", "bad.espf.txt"],
        files: &[("bad.espf.txt", MISSING_SECTIONS)],
    },
    Case {
        name: "check-syntax-error",
        args: &["check", "bad.espf.txt"],
        files: &[("bad.espf.txt", SYNTAX_ERROR)],
    },
    Case {
        name: "check-no-spec-no-example",
        args: &["check"],
        files: &[],
    },
    // --- generation --------------------------------------------------------
    Case {
        name: "gen-valid-file",
        args: &["gen", "blink.espf.txt"],
        files: &[("blink.espf.txt", BLINK), ("app/rust/app.rs", APP)],
    },
    Case {
        name: "new-yes",
        args: &[
            "new", "--yes", "--chip", "esp32c3", "--runtime", "blocking", "myproj",
        ],
        files: &[],
    },
    // --- failure and usage paths -------------------------------------------
    Case {
        name: "build-without-spec",
        args: &["build"],
        files: &[],
    },
    Case {
        name: "usage-unknown-subcommand",
        args: &["nosuchcmd"],
        files: &[],
    },
    Case {
        name: "directory-missing",
        args: &["-C", "/nonexistent/espforge-transcript", "version"],
        files: &[],
    },
    Case {
        name: "new-without-tty",
        args: &["new"],
        files: &[],
    },
    Case {
        name: "examples-browser-without-tty",
        args: &["examples"],
        files: &[],
    },
];

fn snapshot_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("transcripts")
}

/// Run one case in a fresh scratch directory and render its transcript.
fn transcript(case: &Case) -> String {
    let tmp = tempfile::tempdir().expect("create scratch dir");
    for (path, body) in case.files {
        let file = tmp.path().join(path);
        if let Some(parent) = file.parent() {
            fs::create_dir_all(parent).expect("create scratch subdir");
        }
        fs::write(&file, body).expect("write scratch file");
    }

    let out = Command::new(BIN)
        .args(case.args)
        .current_dir(tmp.path())
        // Plain output: no colour, no paging, no terminal.
        .env("TERM", "dumb")
        .env_remove("NO_COLOR")
        .env_remove("CLICOLOR")
        .env_remove("CLICOLOR_FORCE")
        .stdin(Stdio::null())
        .output()
        .expect("run espforge");

    let code = match out.status.code() {
        Some(code) => code.to_string(),
        None => format!("signal {:?}", out.status),
    };

    let mut text = format!("$ espforge {}\nexit: {code}\n", case.args.join(" "));
    text.push_str("--- stdout ---\n");
    text.push_str(&String::from_utf8_lossy(&out.stdout));
    text.push_str("--- stderr ---\n");
    text.push_str(&String::from_utf8_lossy(&out.stderr));

    // Scratch paths must not leak into snapshots.
    if let Some(dir) = tmp.path().to_str() {
        text = text.replace(dir, "<cwd>");
    }
    text
}

#[test]
fn cli_transcripts_are_unchanged() {
    let dir = snapshot_dir();
    let update = std::env::var_os("UPDATE_TRANSCRIPTS").is_some_and(|v| v == "1");
    let mut mismatches = Vec::new();

    for case in CASES {
        let actual = transcript(case);
        let file = dir.join(format!("{}.txt", case.name));
        if update {
            fs::create_dir_all(&dir).expect("create transcript dir");
            fs::write(&file, &actual).expect("write snapshot");
            continue;
        }
        let expected = match fs::read_to_string(&file) {
            Ok(text) => text,
            Err(_) => {
                mismatches.push(format!(
                    "{}: no snapshot at {}",
                    case.name,
                    file.display()
                ));
                continue;
            }
        };
        if expected != actual {
            mismatches.push(format!(
                "{}: transcript changed\n--- expected ---\n{expected}--- actual ---\n{actual}",
                case.name
            ));
        }
    }

    if !mismatches.is_empty() {
        panic!(
            "{} CLI transcript(s) differ. If the change is intended, run \
             `UPDATE_TRANSCRIPTS=1 cargo test -p espforge --test transcripts` \
             and review the diff.\n\n{}",
            mismatches.len(),
            mismatches.join("\n")
        );
    }
}

/// `examples list` is the scripted, piped form; its shape is a contract even
/// though the example corpus may grow (each line is `category/name<TAB>title`).
#[test]
fn examples_list_is_machine_readable() {
    let out = Command::new(BIN)
        .args(["examples", "list"])
        .output()
        .expect("run espforge");
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(!text.is_empty());
    for line in text.lines() {
        let (path, title) = line.split_once('\t').unwrap_or_else(|| {
            panic!("line is not `category/name<TAB>title`: {line:?}");
        });
        assert_eq!(path.split('/').count(), 2, "not `category/name`: {path:?}");
        assert!(!title.is_empty(), "missing title for {path}");
    }
}

/// `doctor` probes the host toolchain, so it is not byte-snapshotted. What is
/// stable is the contract in its header: it reports every check and exits 1
/// when a *required* one fails (cargo, rustc targets), 0 otherwise. An empty
/// `PATH` makes the required checks fail deterministically on any host.
#[test]
fn doctor_reports_without_failing() {
    let out = Command::new(BIN)
        .arg("doctor")
        .env("PATH", "")
        .output()
        .expect("run espforge");
    assert_eq!(out.status.code(), Some(1));
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.starts_with("espforge doctor"), "unexpected: {text}");
    assert!(text.contains("✖ cargo"), "cargo should fail: {text}");
    assert!(text.contains("check(s) failed"), "missing verdict: {text}");
}
