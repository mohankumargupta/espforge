//! The Phase 3 dual-run oracle must accept every file the current emitter
//! produces, not just the synthetic examples in the unit tests.
//!
//! This walks the 24 golden cells and parses each generated Rust/TOML/JSON
//! file through the comparator. When the typed backends land, the same
//! comparator compares old and new producers; this test keeps the "old" side
//! honest in the meantime.

use std::fs;
use std::path::{Path, PathBuf};

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

    assert_eq!(cells, 24, "golden cell count changed");
    assert!(
        rust >= 24 * 4,
        "expected four Rust files per cell, got {rust}"
    );
    assert!(
        toml >= 24 * 3,
        "expected three TOML inputs per cell, got {toml}"
    );
    assert!(
        json >= 24 * 2,
        "expected diagram/manifest per cell, got {json}"
    );
}
