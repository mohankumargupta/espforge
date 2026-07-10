//! Build-time driver-registry codegen (design §9b).
//!
//! Each driver module under `src/components/` / `src/devices/` exports a fixed
//! `pub const DRIVER: &'static dyn Driver`. This script globs those dirs and
//! emits `<group>_gen.rs` into `OUT_DIR` with the `pub mod` declarations and the
//! `Registry::new(&[..])` list, so adding a driver file needs no `mod.rs` edit.
//! No link-time discovery, no new dependencies — the emitted file is plain,
//! inspectable Rust under `target/.../out/`.

use std::fmt::Write as _;
use std::fs;
use std::path::Path;

/// Collect driver module names (filename stems) from `dir`, excluding `mod.rs`.
fn driver_modules(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("rs") {
                continue;
            }
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                if stem != "mod" {
                    names.push(stem.to_string());
                }
            }
        }
    }
    names.sort_unstable();
    names
}

/// Emit `<group>_gen.rs` for one group (components / devices).
///
/// Each driver file is `include!`d into a generated submodule of the same name
/// (so its `pub` items — notably `DRIVER` — are reachable as `led::DRIVER`).
/// We use an absolute `#[path]`-style `include!` because the generated file
/// lives in `OUT_DIR`, not `src/`.
fn emit_group(group: &str, modules: &[String], out_dir: &Path, src: &Path) {
    let mut code = String::new();
    for m in modules {
        let file = src.join(group).join(format!("{m}.rs"));
        let abs = file.canonicalize().unwrap_or(file).display().to_string();
        writeln!(code, "pub mod {m} {{").unwrap();
        writeln!(code, "    include!({abs:?});").unwrap();
        writeln!(code, "}}").unwrap();
    }
    writeln!(code).unwrap();
    code.push_str("use espforge_model::driver::Registry;\n\n");
    code.push_str("/// Auto-generated driver registry (design §9b). Do not edit.\n");
    code.push_str("pub fn registry() -> Registry {\n");
    code.push_str("    Registry::new(&[\n");
    for m in modules {
        writeln!(code, "        {m}::DRIVER,").unwrap();
    }
    code.push_str("    ])\n");
    code.push_str("}\n");

    let out_path = out_dir.join(format!("{group}_gen.rs"));
    fs::write(&out_path, code).expect("write generated registry");
}

fn main() {
    let manifest = env!("CARGO_MANIFEST_DIR");
    let src = Path::new(manifest).join("src");
    let out_dir = Path::new(&std::env::var("OUT_DIR").expect("OUT_DIR")).to_path_buf();

    for group in ["components", "devices"] {
        let dir = src.join(group);
        println!("cargo:rerun-if-changed={}", dir.display());
        let modules = driver_modules(&dir);
        emit_group(group, &modules, &out_dir, &src);
    }
    // The manifest itself (Cargo.toml) can change the set; trigger on it too.
    println!("cargo:rerun-if-changed={}", src.join("lib.rs").display());
}
