//! The golden harness: every declared cell of every embedded example,
//! generated through `espforge-core` in-process and compared byte for byte
//! against `tests/golden/<slug>/`.
//!
//! The matrix is discovered, not listed: `example.toml` beside each embedded
//! example declares its chips and runtimes, and every declared cell must have
//! a golden directory. A golden directory with no declared cell is an error
//! too, so a renamed cell cannot leave a stale tree behind.
//!
//! `UPDATE_GOLDENS=1` rewrites the golden trees from the generator; review the
//! diff before committing it.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use espforge_core::answers::Overrides;
use espforge_core::project::generate_with;
use espforge_core::source::resolve_source;
use espforge_elements::Runtime;

struct Cell {
    category: String,
    example: String,
    chip: String,
    runtime: Runtime,
    slug: String,
}

impl Cell {
    fn example_path(&self) -> String {
        format!("{}/{}", self.category, self.example)
    }
}

fn update_goldens() -> bool {
    std::env::var_os("UPDATE_GOLDENS").is_some_and(|v| v != "0")
}

fn golden_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
}

fn runtime_from(name: &str) -> Runtime {
    match name {
        "blocking" => Runtime::Blocking,
        "embassy" => Runtime::Embassy,
        other => panic!("example.toml names unknown runtime `{other}`"),
    }
}

/// Every declared cell, read from each example's own `example.toml`.
fn declared_cells() -> Vec<Cell> {
    let mut cells = Vec::new();
    for category in espforge_examples::categories() {
        for example in &category.examples {
            let manifest = example
                .files
                .iter()
                .find(|f| f.path == "example.toml")
                .unwrap_or_else(|| {
                    panic!(
                        "example {}/{} has no example.toml; the harness cannot know its cells",
                        category.name, example.name
                    )
                });
            let text = std::str::from_utf8(manifest.contents)
                .unwrap_or_else(|_| panic!("{}/example.toml is not UTF-8", example.name));
            let doc: toml::Value = toml::from_str(text)
                .unwrap_or_else(|e| panic!("{}/example.toml: {e}", example.name));
            let chips = doc
                .get("chips")
                .and_then(toml::Value::as_array)
                .unwrap_or_else(|| panic!("{}/example.toml has no `chips`", example.name))
                .iter()
                .map(|v| {
                    v.as_str()
                        .unwrap_or_else(|| {
                            panic!("{}/example.toml chips must be strings", example.name)
                        })
                        .to_string()
                })
                .collect::<Vec<_>>();
            let runtimes = doc
                .get("runtimes")
                .and_then(toml::Value::as_array)
                .unwrap_or_else(|| panic!("{}/example.toml has no `runtimes`", example.name))
                .iter()
                .map(|v| {
                    v.as_str()
                        .unwrap_or_else(|| {
                            panic!("{}/example.toml runtimes must be strings", example.name)
                        })
                        .to_string()
                })
                .collect::<Vec<_>>();

            for chip in chips {
                for runtime in &runtimes {
                    cells.push(Cell {
                        category: category.name.clone(),
                        example: example.name.clone(),
                        chip: chip.clone(),
                        runtime: runtime_from(runtime),
                        slug: format!("{}.{}.{}.{}", category.name, example.name, chip, runtime),
                    });
                }
            }
        }
    }
    cells
}

fn runtime_name(runtime: Runtime) -> &'static str {
    match runtime {
        Runtime::Blocking => "blocking",
        Runtime::Embassy => "embassy",
    }
}

/// Generate one cell into `out`, optionally against a local runtime checkout.
fn generate_cell_with_runtime(cell: &Cell, out: &Path, runtime_path: Option<PathBuf>) {
    let source =
        resolve_source(None, Some(&cell.example_path()), Path::new(".")).unwrap_or_else(|e| {
            panic!("resolving example {}: {e}", cell.example_path());
        });
    let overrides = Overrides {
        chip: Some(cell.chip.clone()),
        runtime: Some(cell.runtime),
        ..Default::default()
    };
    generate_with(&source, out, &overrides, runtime_path)
        .unwrap_or_else(|e| panic!("generating {}: {e:?}", cell.slug));
}

/// Generate one cell into `out` as checked in (published runtime pin).
fn generate_cell(cell: &Cell, out: &Path) {
    generate_cell_with_runtime(cell, out, None);
}

/// Every file under `root`, relative and sorted.
fn tree(root: &Path) -> BTreeSet<String> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeSet<String>) {
        let mut entries = std::fs::read_dir(dir)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", dir.display()))
            .collect::<std::io::Result<Vec<_>>>()
            .unwrap();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            let path = entry.path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                out.insert(
                    path.strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
            }
        }
    }
    let mut out = BTreeSet::new();
    walk(root, root, &mut out);
    out
}

fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

/// A readable expected/actual diff for the first difference in a file.
fn diff(path: &str, expected: &[u8], actual: &[u8]) -> String {
    if let (Ok(expected), Ok(actual)) = (std::str::from_utf8(expected), std::str::from_utf8(actual))
    {
        let expected_lines: Vec<&str> = expected.lines().collect();
        let actual_lines: Vec<&str> = actual.lines().collect();
        let first = expected_lines
            .iter()
            .zip(&actual_lines)
            .position(|(e, a)| e != a)
            .unwrap_or_else(|| expected_lines.len().min(actual_lines.len()));
        let start = first.saturating_sub(3);
        let end = (first + 4).max(start + 1);
        let mut out = format!("`{path}` differs at line {}:\n", first + 1);
        out.push_str("  expected | actual\n");
        for i in start..end {
            let e = expected_lines.get(i).copied().unwrap_or("<missing line>");
            let a = actual_lines.get(i).copied().unwrap_or("<missing line>");
            let marker = if e != a { ">" } else { " " };
            out.push_str(&format!("{marker} {:>5} | {e:<40} | {a}\n", i + 1));
        }
        if expected.ends_with('\n') != actual.ends_with('\n') {
            out.push_str(&format!(
                "  (trailing newline: expected {}, actual {})\n",
                expected.ends_with('\n'),
                actual.ends_with('\n')
            ));
        }
        out.push_str(&format!(
            "  lengths: expected {} bytes, actual {} bytes\n",
            expected.len(),
            actual.len()
        ));
        return out;
    }
    format!(
        "`{path}` differs (binary): expected {} bytes, actual {} bytes\n",
        expected.len(),
        actual.len()
    )
}

/// Compare the generated tree with the golden tree; panic with the first
/// difference and a readable diff.
fn compare_tree(golden: &Path, actual: &Path) {
    let expected_files = tree(golden);
    let actual_files = tree(actual);

    let missing: Vec<&String> = expected_files.difference(&actual_files).collect();
    let extra: Vec<&String> = actual_files.difference(&expected_files).collect();
    assert!(
        missing.is_empty(),
        "golden has files the generator does not write: {missing:?}\n  golden: {}\n  actual: {}",
        golden.display(),
        actual.display()
    );
    assert!(
        extra.is_empty(),
        "the generator writes files the golden does not have: {extra:?}\n  golden: {}\n  actual: {}",
        golden.display(),
        actual.display()
    );

    for path in &expected_files {
        let expected = read(&golden.join(path));
        let got = read(&actual.join(path));
        if expected != got {
            panic!(
                "generated project is not byte-identical:\n{}",
                diff(path, &expected, &got)
            );
        }
    }
}

fn write_golden(from: &Path, to: &Path) {
    if to.exists() {
        std::fs::remove_dir_all(to).unwrap();
    }
    std::fs::create_dir_all(to).unwrap();
    for path in tree(from) {
        let target = to.join(&path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::copy(from.join(&path), &target).unwrap();
    }
}

#[test]
fn every_declared_cell_is_byte_identical_to_its_golden() {
    let cells = declared_cells();
    assert!(!cells.is_empty(), "no golden cells are declared");

    let update = update_goldens();
    let mut declared = BTreeSet::new();
    for cell in &cells {
        declared.insert(cell.slug.clone());
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("project");
        generate_cell(cell, &out);

        // The 11 emitted artifacts plus `src/app.rs`, the copy of the ESPF
        // source, and an OTA project's generated `partitions.csv`.
        const BASE_FILES: [&str; 13] = [
            "build.rs",
            "Cargo.toml",
            ".cargo/config.toml",
            ".cargo/esp-config.toml",
            "diagram.json",
            ".espforge/source.espf.txt",
            "README.txt",
            "rust-toolchain.toml",
            "src/app.rs",
            "src/bin/main.rs",
            "src/generated.rs",
            "src/lib.rs",
            "wokwi.toml",
        ];
        let files = tree(&out);
        for path in BASE_FILES {
            assert!(
                files.contains(path),
                "{}: missing `{path}` ({files:?})",
                cell.slug
            );
        }
        let extra: Vec<&String> = files
            .iter()
            .filter(|path| {
                !BASE_FILES.contains(&path.as_str())
                    && !matches!(
                        path.as_str(),
                        "chip.json" | "chip.wasm" | "partitions.csv"
                    )
            })
            .collect();
        assert!(
            extra.is_empty(),
            "{}: unexpected generated files {extra:?}",
            cell.slug
        );

        let golden = golden_root().join(&cell.slug);
        if update {
            write_golden(&out, &golden);
            eprintln!("updated {}", golden.display());
        } else {
            assert!(
                golden.is_dir(),
                "declared cell `{}` has no golden at {}; run with UPDATE_GOLDENS=1",
                cell.slug,
                golden.display()
            );
            compare_tree(&golden, &out);
        }
    }

    // A golden tree with no declared cell is an error, not silent coverage.
    let root = golden_root();
    if root.is_dir() {
        for entry in std::fs::read_dir(&root).unwrap() {
            let entry = entry.unwrap();
            if !entry.path().is_dir() {
                continue;
            }
            let slug = entry.file_name().to_string_lossy().into_owned();
            assert!(
                declared.contains(&slug),
                "golden `{slug}` is not a declared cell; declare it in example.toml or delete the tree"
            );
        }
    }

    // The cell the slice is about, named here so its presence is explicit.
    assert!(
        declared.contains(&format!(
            "01.Basics.helloworld.esp32c3.{}",
            runtime_name(Runtime::Blocking)
        )),
        "the helloworld esp32c3/blocking cell must stay declared"
    );
}

/// The targets `rustup` has installed, so the gate below can say what it could
/// not compile instead of failing on a missing toolchain.
fn installed_targets() -> BTreeSet<String> {
    let output = std::process::Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
        .expect("the compile gate needs `rustup` on PATH");
    assert!(
        output.status.success(),
        "`rustup target list --installed` failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect()
}

/// The `[build] target = "..."` a generated project compiles for.
fn build_target(project: &Path) -> String {
    let config = std::fs::read_to_string(project.join(".cargo/config.toml"))
        .expect("a generated project has .cargo/config.toml");
    config
        .lines()
        .map(str::trim)
        .find_map(|line| line.strip_prefix("target = "))
        .map(|value| value.trim_matches('"').to_string())
        .expect("the generated .cargo/config.toml names a build target")
}

/// The compile gate: every generated project typechecks for its chip.
///
/// Ignored in the default run because it shells out to `cargo` per cell: it
/// needs the RISC-V target and the dependencies in the cargo cache. This is
/// what CI's `generated-projects` job runs (`-- --ignored`), and the invariant
/// this repo lacked: an emitter change is compiled here instead of being
/// discovered by a user running `espforge build`.
///
/// Cells whose target is not installed are skipped and named, never silently
/// counted: the `esp32s3` cells are xtensa and need a different toolchain.
#[test]
#[ignore = "compiles every generated project; run with `-- --ignored`"]
fn every_generated_project_compiles() {
    let workspace = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("the espforge crate lives in the workspace root")
        .to_path_buf();
    // Generated projects pin the runtime from crates.io; point them at this
    // checkout so the gate compiles the emitter it is testing.
    let runtime = workspace.join("espforge-runtime");
    // One cache for every cell: the first build compiles the dependencies.
    let target_dir = workspace.join("target/golden-check");
    let installed = installed_targets();

    let mut compiled = Vec::new();
    let mut skipped = Vec::new();
    for cell in &declared_cells() {
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("project");
        generate_cell_with_runtime(cell, &out, Some(runtime.clone()));

        let target = build_target(&out);
        if !installed.contains(&target) {
            skipped.push(format!("{} ({target})", cell.slug));
            continue;
        }

        let output = std::process::Command::new(env!("CARGO"))
            .args(["build", "--quiet"])
            .current_dir(&out)
            .env("CARGO_TARGET_DIR", &target_dir)
            .output()
            .unwrap_or_else(|e| panic!("running cargo build for {}: {e}", cell.slug));
        assert!(
            output.status.success(),
            "{} does not compile for {target}:\n{}",
            cell.slug,
            String::from_utf8_lossy(&output.stderr)
        );
        compiled.push(cell.slug.clone());
    }

    assert!(
        !compiled.is_empty(),
        "no generated project was compiled; skipped: {skipped:?}"
    );
    eprintln!(
        "compiled {} generated projects; skipped {} for a missing target: {}",
        compiled.len(),
        skipped.len(),
        skipped.join(", ")
    );
}
