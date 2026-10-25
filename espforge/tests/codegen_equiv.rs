//! The Phase 3 dual-run oracle must accept every file the current emitter
//! produces, not just the synthetic examples in the unit tests.
//!
//! This walks every golden cell and parses each generated Rust/TOML/JSON file
//! through the comparator. When the typed backends landed, the same comparator
//! compared old and new producers; this test keeps the pinned trees themselves
//! honest — a cell whose Rust stops parsing fails here even when its bytes were
//! regenerated deliberately.

use std::fs;
use std::path::{Path, PathBuf};

/// Cells pinned in `tests/golden`, every generating example on both chips.
/// A mismatch means goldens were added or removed without updating the sweep.
const CELLS: usize = 30;

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("golden")
}

#[test]
fn every_golden_file_parses_through_the_comparator() {
    let mut cells = 0;
    let mut rust = 0;
    let mut toml = 0;
    let mut json = 0;

    for cell in fs::read_dir(golden_dir()).expect("read golden dir") {
        let cell = cell.expect("read cell").path();
        if !cell.is_dir() {
            continue;
        }
        cells += 1;
        let mut stack = vec![cell];
        while let Some(dir) = stack.pop() {
            for entry in fs::read_dir(&dir).expect("read cell entry") {
                let path = entry.expect("read entry").path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                let bytes =
                    fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
                // Binary assets (e.g. `chip.wasm`) are copied verbatim and
                // have no semantic backend.
                let Ok(text) = String::from_utf8(bytes) else {
                    continue;
                };
                match path.extension().and_then(|e| e.to_str()) {
                    Some("rs") => {
                        espforge_emit::codegen::equiv::same_rust(&text, &text)
                            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                        rust += 1;
                    }
                    Some("toml") => {
                        espforge_emit::codegen::equiv::same_toml(&text, &text)
                            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                        toml += 1;
                    }
                    Some("json") => {
                        espforge_emit::codegen::equiv::same_json(&text, &text)
                            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
                        json += 1;
                    }
                    _ => {}
                }
            }
        }
    }

    assert_eq!(cells, CELLS, "golden cell count changed");
    assert!(
        rust >= cells * 4,
        "expected four Rust files per cell, got {rust}"
    );
    assert!(
        toml >= cells * 3,
        "expected three TOML inputs per cell, got {toml}"
    );
    assert!(
        json >= cells * 2,
        "expected diagram/manifest per cell, got {json}"
    );
}
