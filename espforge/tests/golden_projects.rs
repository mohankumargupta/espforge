//! Golden generated projects: one checked-in tree per (example, chip,
//! runtime), compared byte for byte against what `espforge gen` produces
//! today.
//!
//! This is the Phase 0 contract freeze from `extreme.md` §6.3/§7: generated
//! project layout and ownership semantics are expected to survive the
//! refactor. Today the first time a template change is compiled is a real
//! user's `espforge build`; after this test, a change to the emitter shows up
//! as a reviewed diff in `tests/golden/`.
//!
//! Regenerate after a deliberate change:
//!
//! ```text
//! UPDATE_GOLDENS=1 cargo test -p espforge --test golden_projects
//! ```
//!
//! The byte comparison cannot prove the output *compiles*; that is
//! `generated_projects_compile` below, which is `#[ignore]`d because it spawns
//! cargo (minutes, network for the crates the runtime needs). CI runs it
//! explicitly:
//!
//! ```text
//! cargo test -p espforge --test golden_projects -- --ignored
//! ```

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const BIN: &str = env!("CARGO_BIN_EXE_espforge");

/// Example × chip × runtime cells that are pinned as goldens.
const MATRIX: &[Cell] = &[
    Cell::new("01.Basics/helloworld", "esp32c3", "blocking"),
    Cell::new("01.Basics/helloworld", "esp32c3", "embassy"),
    Cell::new("01.Basics/helloworld", "esp32s3", "blocking"),
    Cell::new("01.Basics/helloworld", "esp32s3", "embassy"),
    Cell::new("01.Basics/blink", "esp32c3", "blocking"),
    Cell::new("01.Basics/blink", "esp32c3", "embassy"),
    Cell::new("01.Basics/blink", "esp32s3", "blocking"),
    Cell::new("01.Basics/blink", "esp32s3", "embassy"),
    Cell::new("02.Digital/button", "esp32c3", "blocking"),
    Cell::new("02.Digital/button", "esp32c3", "embassy"),
    Cell::new("02.Digital/button", "esp32s3", "blocking"),
    Cell::new("02.Digital/button", "esp32s3", "embassy"),
    // Bus coverage. The async examples pin the embassy runtime; the
    // blocking ones pin `blocking`, so the goldens exercise both mains.
    Cell::new("04.Communication/spi_example", "esp32c3", "blocking"),
    Cell::new("04.Communication/spi_example", "esp32s3", "blocking"),
    Cell::new("04.Communication/uart_example", "esp32c3", "blocking"),
    Cell::new("04.Communication/uart_example", "esp32s3", "blocking"),
    Cell::new("04.Communication/i2c_example", "esp32c3", "blocking"),
    Cell::new("04.Communication/i2c_example", "esp32s3", "blocking"),
    Cell::new("04.Communication/i2c_scanner", "esp32c3", "blocking"),
    Cell::new("04.Communication/i2c_scanner", "esp32s3", "blocking"),
    Cell::new("04.Communication/spi_example_async", "esp32c3", "embassy"),
    Cell::new("04.Communication/spi_example_async", "esp32s3", "embassy"),
    Cell::new("04.Communication/uart_example_async", "esp32c3", "embassy"),
    Cell::new("04.Communication/uart_example_async", "esp32s3", "embassy"),
];

/// Cells compiled by `generated_projects_compile`: every example whose
/// `app.rs` matches its runtime, on a RISC-V chip.
///
/// The esp32s3 cells are deliberately not here: they need the `esp` Xtensa
/// toolchain from espup, which a plain rustup install does not provide. They
/// stay covered by the byte-exact goldens above; add them to this list once
/// the CI job installs espup.
///
/// A blocking example cannot be forced onto embassy (its `app.rs` is
/// blocking), and an async example cannot be forced onto blocking. The matrix
/// pairs each example with the runtime its app is written for; the embassy
/// fixture additionally covers the async path for a project with no example
/// app.
const COMPILE_MATRIX: &[Cell] = &[
    Cell::new("01.Basics/helloworld", "esp32c3", "blocking"),
    Cell::new("01.Basics/blink", "esp32c3", "blocking"),
    Cell::new("02.Digital/button", "esp32c3", "blocking"),
    Cell::new("04.Communication/spi_example", "esp32c3", "blocking"),
    Cell::new("04.Communication/uart_example", "esp32c3", "blocking"),
    Cell::new("04.Communication/i2c_example", "esp32c3", "blocking"),
    Cell::new("04.Communication/i2c_scanner", "esp32c3", "blocking"),
    Cell::new("02.Digital/button_async", "esp32c3", "embassy"),
    Cell::new("04.Communication/i2c_example_async", "esp32c3", "embassy"),
    Cell::new("04.Communication/spi_example_async", "esp32c3", "embassy"),
    Cell::new("04.Communication/uart_example_async", "esp32c3", "embassy"),
    Cell::fixture("embassy_app", "esp32c3", "embassy"),
];

/// Where a cell's ESPF spec and app logic come from.
enum Source {
    /// An embedded example, generated with `--example`.
    Example(&'static str),
    /// A local fixture under `tests/fixtures/`, with its own `app/rust/app.rs`.
    Fixture(&'static str),
}

struct Cell {
    source: Source,
    chip: &'static str,
    runtime: &'static str,
}

impl Cell {
    const fn new(example: &'static str, chip: &'static str, runtime: &'static str) -> Self {
        Self {
            source: Source::Example(example),
            chip,
            runtime,
        }
    }

    const fn fixture(name: &'static str, chip: &'static str, runtime: &'static str) -> Self {
        Self {
            source: Source::Fixture(name),
            chip,
            runtime,
        }
    }

    /// Directory name under `tests/golden/`, e.g. `01.Basics.helloworld.esp32c3.blocking`.
    fn slug(&self) -> String {
        let name = match &self.source {
            Source::Example(example) => example.replace('/', "."),
            Source::Fixture(name) => format!("fixture.{name}"),
        };
        format!("{}.{}.{}", name, self.chip, self.runtime)
    }
}

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
}

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}

/// Copy a fixture project (spec + app logic) into the scratch dir.
fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap_or_else(|e| panic!("create {}: {e}", to.display()));
    for entry in fs::read_dir(from).unwrap_or_else(|e| panic!("read {}: {e}", from.display())) {
        let path = entry.expect("read fixture entry").path();
        let dest = to.join(path.file_name().expect("file name"));
        if path.is_dir() {
            copy_tree(&path, &dest);
        } else {
            fs::copy(&path, &dest)
                .unwrap_or_else(|e| panic!("copy {}: {e}", path.display()));
        }
    }
}

/// Files of a generated project, keyed by `/`-separated relative path, sorted.
/// Values are raw bytes: a Wokwi chip ships a `chip.wasm`.
fn tree(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        let mut entries: Vec<_> = fs::read_dir(dir)
            .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
            .map(|entry| entry.expect("read dir entry").path())
            .collect();
        entries.sort();
        for path in entries {
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                let rel = path
                    .strip_prefix(root)
                    .expect("path under root")
                    .to_string_lossy()
                    .replace('\\', "/");
                let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {rel}: {e}"));
                out.insert(rel, bytes);
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

/// Show a generated file in a diff: text as-is, binary by size.
fn describe(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_owned(),
        Err(_) => format!("<{} bytes of binary data>\n", bytes.len()),
    }
}

/// Generate one cell into `scratch` and return the file tree of `scratch/out`.
fn generate(cell: &Cell, scratch: &Path, extra: &[&str]) -> BTreeMap<String, Vec<u8>> {
    let mut cmd = Command::new(BIN);
    match &cell.source {
        Source::Example(example) => {
            cmd.args(["gen", "--example", example, "--out", "out"]);
        }
        Source::Fixture(name) => {
            copy_tree(&fixtures_dir().join(name), scratch);
            cmd.args(["gen", "example.espf.txt", "--out", "out"]);
        }
    }
    cmd.args(["--chip", cell.chip, "--runtime", cell.runtime])
        .args(extra)
        .current_dir(scratch)
        .env("TERM", "dumb")
        .env_remove("NO_COLOR")
        .stdin(Stdio::null());
    let out = cmd.output().expect("run espforge gen");
    assert!(
        out.status.success(),
        "gen {} failed: \n{}{}",
        cell.slug(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    tree(&scratch.join("out"))
}

#[test]
fn golden_projects_are_unchanged() {
    let root = golden_dir();
    let update = std::env::var_os("UPDATE_GOLDENS").is_some_and(|v| v == "1");
    let mut mismatches = Vec::new();

    for cell in MATRIX {
        let scratch = tempfile::tempdir().expect("create scratch dir");
        let actual = generate(cell, scratch.path(), &[]);
        let case_dir = root.join(cell.slug());

        if update {
            let _ = fs::remove_dir_all(&case_dir);
            for (rel, body) in &actual {
                let file = case_dir.join(rel);
                fs::create_dir_all(file.parent().expect("parent")).expect("create dir");
                fs::write(&file, body).expect("write golden");
            }
            continue;
        }

        let expected = match fs::read_dir(&case_dir) {
            Ok(_) => tree(&case_dir),
            Err(_) => {
                mismatches.push(format!("{}: no golden tree", cell.slug()));
                continue;
            }
        };

        for (rel, body) in &expected {
            match actual.get(rel) {
                None => mismatches.push(format!("{}: generated project lost {rel}", cell.slug())),
                Some(got) if got != body => mismatches.push(format!(
                    "{}: {rel} changed\n--- expected ---\n{}--- actual ---\n{}",
                    cell.slug(),
                    describe(body),
                    describe(got)
                )),
                Some(_) => {}
            }
        }
        for rel in actual.keys() {
            if !expected.contains_key(rel) {
                mismatches.push(format!("{}: generated project gained {rel}", cell.slug()));
            }
        }
    }

    // A golden tree nobody generates any more is dead weight.
    if let Ok(entries) = fs::read_dir(&root) {
        let slugs: Vec<String> = MATRIX.iter().map(Cell::slug).collect();
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if entry.path().is_dir() && !slugs.contains(&name) {
                mismatches.push(format!("{name}: golden tree without a matrix cell"));
            }
        }
    }

    if !mismatches.is_empty() {
        panic!(
            "{} golden difference(s). If the change is intended, run \
             `UPDATE_GOLDENS=1 cargo test -p espforge --test golden_projects` and review \
             the diff.\n\n{}",
            mismatches.len(),
            mismatches.join("\n")
        );
    }
}

/// Compile the generated project, not just compare its bytes. This is the
/// invariant the repo never had: `espforge build` used to be the first compile
/// of a template change.
///
/// `#[ignore]`d because it needs the network (crates.io) and minutes of cargo
/// time. Run with `-- --ignored`, as CI does.
#[test]
#[ignore = "spawns cargo; run with -- --ignored"]
fn generated_projects_compile() {
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .join("espforge-runtime");
    // One shared target dir, so the four cells reuse dependency builds.
    let target_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .join("target")
        .join("golden-check");

    let mut failures = Vec::new();
    for cell in COMPILE_MATRIX {
        let scratch = tempfile::tempdir().expect("create scratch dir");
        let runtime_arg = runtime.to_string_lossy().into_owned();
        generate(cell, scratch.path(), &["--runtime-path", &runtime_arg]);

        let out = Command::new("cargo")
            .args(["check", "--quiet"])
            .current_dir(scratch.path().join("out"))
            .env("CARGO_TARGET_DIR", &target_dir)
            .output()
            .expect("run cargo check");
        if !out.status.success() {
            failures.push(format!(
                "{}: cargo check failed\n{}{}",
                cell.slug(),
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr)
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "{} of {} generated project(s) do not compile:\n\n{}",
        failures.len(),
        COMPILE_MATRIX.len(),
        failures.join("\n")
    );
}
