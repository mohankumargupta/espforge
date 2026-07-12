//! Code generation emitters (ADR-005). Each emitter is a pure
//! `fn(&DeviceTree) -> Result<Vec<Artifact>>`; the `write` step persists them
//! and records the ownership manifest (ADR-001).

pub mod manifest;
pub mod rust;
pub mod scaffold;
pub mod wokwi;

use anyhow::Result;
use espforge_model::ir::DeviceTree;
use espforge_model::value::{Artifact, Ownership};
use std::collections::BTreeMap;
use std::path::Path;

use manifest::{emit_manifest, emit_readme, owned_paths};

/// Run all emitters against the resolved IR and return every artifact espforge
/// owns (plus the manifest). Does not touch the filesystem.
pub fn generate(ir: &DeviceTree) -> Result<Vec<Artifact>> {
    let mut out = rust::emit(ir)?;
    let owned = owned_paths(&out);
    out.push(Artifact::owned("README.txt", emit_readme(ir, &owned)));
    out.push(emit_manifest(&out)?);
    Ok(out)
}

/// Write artifacts into `out_dir`, respecting ownership (ADR-001):
/// - espforge-owned files are overwritten (regeneration), unless a prior
///   manifest shows the file was edited (drift) — then we refuse rather than
///   clobber. This is the enforcement-grade guarantee.
/// - Files espforge does not own (esp-generate scaffold, user overrides) are
///   never written here.
pub fn write(out_dir: &Path, artifacts: &[Artifact]) -> Result<()> {
    let prev = read_prev_manifest(out_dir)?;
    for a in artifacts {
        let path = out_dir.join(&a.path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        match a.ownership {
            Ownership::NotOwned => continue,
            Ownership::SeedOnce => {
                // User-owned skeleton: write only if absent; never clobber.
                if !path.exists() {
                    std::fs::write(&path, &a.content)?;
                }
            }
            Ownership::Owned => {
                // Regenerated every build; refuse to clobber an edited file
                // (drift detection, ADR-001).
                if let Some(prev_checksum) = prev.get(&a.path) {
                    if let Ok(existing) = std::fs::read_to_string(&path) {
                        if checksum(&existing) != *prev_checksum && existing != a.content {
                            anyhow::bail!(
                                "refusing to overwrite edited file `{}` (drift detected). \
                                 Restore it or delete it to regenerate.",
                                a.path
                            );
                        }
                    }
                }
                std::fs::write(&path, &a.content)?;
            }
        }
    }
    Ok(())
}

fn read_prev_manifest(out_dir: &Path) -> Result<BTreeMap<String, String>> {
    let p = out_dir.join(".espforge/manifest.json");
    match std::fs::read_to_string(&p) {
        Ok(s) => Ok(serde_json::from_str(&s)?),
        Err(_) => Ok(BTreeMap::new()),
    }
}

fn checksum(content: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in content.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}
