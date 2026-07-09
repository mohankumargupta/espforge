//! espforge-examples: curated sample projects, baked into the binary at compile
//! time (v1 heritage, per design §17.3). The example set is a closed, explicit,
//! versioned set — no ambient filesystem discovery (Zen of espforge: explicit
//! over implicit). CI integration gate (ADR-010/011).
//!
//! Each example lives under `examples/<name>/` and follows the v1 tree shape:
//!   - `<name>.yaml`     — the v2 project spec (source of truth)
//!   - `app/rust/app.rs` — user-owned app logic (matches the generated skeleton)
//!   - `diagram.json`    — optional wokwi diagram
//! `create` reads a chosen example out of `EXAMPLES_DIR` and copies it into a new
//! project folder; `build` then runs the pipeline on the copied spec.

use include_dir::include_dir;
/// Re-exported so `espforge` can name the embedded-dir type without depending on
/// `include_dir` directly.
pub use include_dir::Dir;

/// The embedded example tree: compiled from `examples/` next to this crate.
pub static EXAMPLES_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/examples");

/// Statically known example names (the `examples/<name>` directories).
/// Excludes `broken`, which exists only to exercise the validation path.
pub const EXAMPLE_NAMES: &[&str] = &["blink", "display"];

/// Whether `name` is a known example (a top-level dir in the embedded tree).
pub fn is_example(name: &str) -> bool {
    EXAMPLES_DIR.get_dir(name).is_some()
}

/// Fetch a single embedded asset for `example` by its full in-tree path, e.g.
/// `blink/blink.yaml` or `blink/app/rust/app.rs`. Returns `None` if absent.
pub fn asset(example: &str, rel: &str) -> Option<&'static [u8]> {
    let path = format!("{example}/{rel}");
    EXAMPLES_DIR.get_file(&path).map(|f| f.contents())
}

/// Return the example's project spec (the embedded YAML whose body contains
/// `espforge:`). The filename need not match the example name — this decouples
/// the on-disk name (e.g. `ssd1306.yaml`) from the example key (`display`).
pub fn example_spec(example: &str) -> Option<&'static [u8]> {
    let dir = EXAMPLES_DIR.get_dir(example)?;
    dir.files()
        .filter(|f| f.path().extension().and_then(|e| e.to_str()) == Some("yaml"))
        .find(|f| {
            let text = String::from_utf8_lossy(f.contents());
            text.contains("espforge:")
        })
        .map(|f| f.contents())
}
