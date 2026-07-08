//! The human-facing `README.txt` contract and the enforcement-grade ownership
//! manifest (ADR-001).
//!
//! The `README.txt` tells the user which files are theirs to edit and which are
//! machine-owned. The manifest records the exact set of owned files (with
//! checksums of their content at emit time) so a future `build` can detect
//! drift: if an owned file no longer matches its recorded checksum, espforge
//! refuses to clobber the user's edit rather than silently overwriting.

use anyhow::Result;
use espforge_model::ir::DeviceTree;
use espforge_model::value::{Artifact, Ownership};
use std::collections::BTreeMap;

/// Files espforge emits and owns (relative paths). Mirrors the Rust emitter's
/// output plus the manifest itself.
pub fn owned_paths(artifacts: &[Artifact]) -> Vec<String> {
    artifacts
        .iter()
        .filter(|a| a.ownership == Ownership::Owned)
        .map(|a| a.path.clone())
        .collect()
}

pub fn emit_readme(ir: &DeviceTree, owned: &[String]) -> String {
    let name = ir.meta.name.clone().unwrap_or_else(|| "espforge-project".into());
    let owned_list = owned
        .iter()
        .map(|p| format!("  - {p}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        r#"espforge v2 generated project: {name}
==========================================

SOURCE OF TRUTH
  This project is GENERATED from a spec. Do not edit the machine-owned
  files below — they are regenerated every `espforge build`. Edit only
  the user-owned inputs.

USER-OWNED (edit freely):
  - The project YAML you passed to `espforge build`
  - src/app.rs            (your logic: setup/forever)
  - dependencies.toml     (extra crates; merged into Cargo.toml)
  - .cargo/config.toml    (override; replaces the generated base if present)
  - diagram.json          (wokwi override; replaces the generated base if present)

MACHINE-OWNED by espforge (DO NOT EDIT; regenerated every build):
{owned_list}

REGENERATE:
  espforge build <your.yaml> --out <this dir>
"#
    )
}

/// The ownership manifest: maps each owned file to a checksum of the content
/// espforge wrote. Stored at `.espforge/manifest.json` in the output dir.
pub fn emit_manifest(artifacts: &[Artifact]) -> Result<Artifact> {
    let mut map: BTreeMap<String, String> = BTreeMap::new();
    for a in artifacts.iter().filter(|a| a.ownership == Ownership::Owned) {
        map.insert(a.path.clone(), checksum(&a.content));
    }
    let json = serde_json::to_string_pretty(&map)?;
    Ok(Artifact::owned(".espforge/manifest.json", json))
}

fn checksum(content: &str) -> String {
    // FNV-1a 64-bit: dependency-free, sufficient to detect accidental drift
    // (not a security hash).
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in content.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}
