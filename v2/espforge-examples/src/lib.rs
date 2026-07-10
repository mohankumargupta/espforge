//! espforge-examples: curated sample projects, baked into the binary at compile
//! time (v1 heritage, per design §17.3). The example set is a closed, explicit,
//! versioned set — no ambient filesystem discovery (Zen of espforge: explicit
//! over implicit). CI integration gate (ADR-010/011).
//!
//! Examples follow the v1 folder convention: numbered category folders
//! (`01.Basics/`, `06.Displays/`, …) each holding leaf example subfolders named
//! by the example key, e.g. `01.Basics/blink`, `06.Displays/display`. Each leaf
//! follows the v1 tree shape:
//!   - `<name>.yaml`     — the v2 project spec (source of truth)
//!   - `app/rust/app.rs` — user-owned app logic (matches the generated skeleton)
//!   - `diagram.json`    — optional wokwi diagram
//! The category folder is organizational only; an example is resolved by its
//! leaf folder name (v1 behaviour: `create blink` matches `01.Basics/blink`).
//! `create` copies the matched leaf into a new project folder; `build` then runs
//! the pipeline on the copied spec.

use include_dir::include_dir;

/// Re-exported so `espforge` can name the embedded-dir type without depending on
/// `include_dir` directly.
pub use include_dir::Dir;

/// The embedded example tree: compiled from `examples/` next to this crate.
pub static EXAMPLES_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/examples");

/// Walk the embedded tree and collect every leaf example as
/// `(rel_path, dir)`, where `rel_path` is the category-qualified path verbatim
/// from the tree root (e.g. `01.Basics/blink`, `06.Displays/display`). Category
/// folders are organizational only but are preserved verbatim so the picker and
/// error lists can show them (design §17.1). `rel_path` is derived straight from
/// `dir.path()` relative to `EXAMPLES_DIR`, so it cannot drift from the real
/// folder layout.
fn walk_examples<'a>(dir: &'a Dir<'a>, out: &mut Vec<(String, &'a Dir<'a>)>) {
    // A leaf example dir has a spec as an *immediate* child (a .yaml containing
    // `espforge:`). We check immediate files only, so a parent category folder
    // (e.g. `01.Basics`) is not mistaken for an example.
    let is_example = dir.files().any(|f| {
        f.path().extension().and_then(|e| e.to_str()) == Some("yaml")
            && String::from_utf8_lossy(f.contents()).contains("espforge:")
    });
    if is_example {
        let rel = dir
            .path()
            .strip_prefix(EXAMPLES_DIR.path())
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        out.push((rel, dir));
        return; // don't descend into an example leaf
    }
    for d in dir.dirs() {
        walk_examples(d, out);
    }
}

/// List known example names as category-qualified paths verbatim from the
/// embedded tree (e.g. `01.Basics/blink`, `06.Displays/display`). Sorted for
/// stable display in the picker and error listing (design §17.1).
pub fn example_names() -> Vec<String> {
    let mut all = Vec::new();
    walk_examples(&EXAMPLES_DIR, &mut all);
    let mut names: Vec<String> = all.into_iter().map(|(rel, _)| rel).collect();
    names.sort_unstable();
    names
}

/// Return the embedded leaf directory for a known example, resolved either by
/// its full category-qualified path (`01.Basics/blink`) or its bare leaf name
/// (`blink`). The exact path is preferred; bare-leaf is a fallback so the common
/// `create blink` invocation keeps working. Closed, explicit set — no fuzzy
/// matching.
pub fn find_example(name: &str) -> Option<&'static Dir<'static>> {
    let mut all = Vec::new();
    walk_examples(&EXAMPLES_DIR, &mut all);
    all.iter()
        .find(|(rel, d)| rel == name || d.path().file_name().and_then(|n| n.to_str()) == Some(name))
        .map(|(_, d)| *d)
}

/// Whether `name` is a known example (resolvable by leaf folder name).
pub fn is_example(name: &str) -> bool {
    find_example(name).is_some()
}

/// Recursively collect every file under `dir` (include_dir's `files()` is
/// non-recursive, so nested assets like `app/rust/app.rs` are missed otherwise).
fn walk_files<'a>(dir: &'a Dir<'a>, out: &mut Vec<&'a include_dir::File<'a>>) {
    for f in dir.files() {
        out.push(f);
    }
    for d in dir.dirs() {
        walk_files(d, out);
    }
}

/// Fetch a single embedded asset for `example` by its relative in-tree path,
/// e.g. `app/rust/app.rs` or `blink.yaml`. Matched by path tail so the category
/// prefix (`01.Basics/`) is irrelevant.
pub fn asset(example: &str, rel: &str) -> Option<&'static [u8]> {
    let dir = find_example(example)?;
    let mut files = Vec::new();
    walk_files(dir, &mut files);
    files
        .into_iter()
        .find(|f| f.path().to_string_lossy().ends_with(rel))
        .map(|f| f.contents())
}

/// Return the example's project spec (the embedded YAML whose body contains
/// `espforge:`). The filename need not match the example name — this decouples
/// the on-disk name (e.g. `ssd1306.yaml`) from the example key (`display`).
pub fn example_spec(example: &str) -> Option<&'static [u8]> {
    let dir = find_example(example)?;
    let mut files = Vec::new();
    walk_files(dir, &mut files);
    files
        .into_iter()
        .filter(|f| f.path().extension().and_then(|e| e.to_str()) == Some("yaml"))
        .find(|f| {
            let text = String::from_utf8_lossy(f.contents());
            text.contains("espforge:")
        })
        .map(|f| f.contents())
}
