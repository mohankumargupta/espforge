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

/// Derive the list of example names from the embedded tree: every leaf
/// directory that contains a project spec (a `.yaml` with `espforge:`), resolved
/// by leaf folder name. Excludes `broken`, which exists only to exercise the
/// validation path — its dir has no spec. This is derived rather than a hand
/// maintained list so adding a template (e.g. `02.Digital/button`) needs no code
/// change.
pub fn example_names() -> Vec<&'static str> {
    fn walk(dir: &Dir<'static>, out: &mut Vec<&'static str>) {
        // A leaf example dir has a spec as an *immediate* child (a .yaml
        // containing `espforge:`). We check immediate files only, so a parent
        // category folder (e.g. `01.Basics`) is not mistaken for an example.
        let is_example = dir.files().any(|f| {
            f.path().extension().and_then(|e| e.to_str()) == Some("yaml")
                && String::from_utf8_lossy(f.contents()).contains("espforge:")
        });
        if is_example {
            if let Some(name) = dir.path().file_name().and_then(|n| n.to_str()) {
                out.push(name);
            }
            return; // don't descend into an example leaf
        }
        for d in dir.dirs() {
            walk(d, out);
        }
    }
    let mut out = Vec::new();
    walk(&EXAMPLES_DIR, &mut out);
    out.sort_unstable();
    out
}

/// Return the embedded leaf directory for a known example, resolved by **leaf
/// folder name** anywhere in the tree (v1 behaviour). Returns `None` for an
/// unknown example — a closed, explicit set with no fuzzy matching.
pub fn find_example(name: &str) -> Option<&'static Dir<'static>> {
    fn walk<'a>(dir: &'a Dir<'a>, name: &str) -> Option<&'a Dir<'a>> {
        if dir.path().file_name().and_then(|n| n.to_str()) == Some(name) {
            return Some(dir);
        }
        for d in dir.dirs() {
            if let Some(found) = walk(d, name) {
                return Some(found);
            }
        }
        None
    }
    walk(&EXAMPLES_DIR, name)
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
