//! Build script for `espforge-examples`.
//!
//! The example tree is baked into the binary at compile time via
//! `include_dir!("$CARGO_MANIFEST_DIR/examples")` in `src/lib.rs`. Cargo only
//! tracks `.rs` sources by default, so edits/additions/removals to the example
//! data (`*.yaml`, `app/rust/app.rs`, `diagram.json`, …) would otherwise be
//! invisible to the build and require a `cargo clean` to pick up.
//!
//! Emitting `rerun-if-changed` for every file makes content edits rebuild, and
//! the directory entry makes additions/removals rebuild (which re-walks and
//! picks up the new file on the next run).

use std::path::{Path, PathBuf};

fn main() {
    let root = Path::new("examples");
    println!("cargo:rerun-if-changed={}", root.display());
    for path in walk_dir(root) {
        println!("cargo:rerun-if-changed={}", path.display());
    }
}

fn walk_dir(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            out.push(path.clone());
            if path.is_dir() {
                out.extend(walk_dir(&path));
            }
        }
    }
    out
}
