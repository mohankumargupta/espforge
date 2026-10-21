//! The ownership manifest used for drift detection.
//!
//! `.espforge/manifest.json`:
//!
//! ```json
//! { "template_version": "0.1.9", "files": { "Cargo.toml": "<fnv1a>", ... } }
//! ```
//!
//! `files` maps each `Owned` file to a checksum of what espforge wrote.

use anyhow::Result;
use espforge_model::{Artifact, Ownership};
use serde_json::json;
use std::collections::BTreeMap;

/// Build the manifest artifact for every `Owned` artifact in `artifacts`.
pub fn emit_manifest(artifacts: &[Artifact], template_version: &str) -> Result<Artifact> {
    let files: BTreeMap<String, String> = artifacts
        .iter()
        .filter(|a| a.ownership == Ownership::Owned)
        .map(|a| (a.path.clone(), checksum(&a.content)))
        .collect();
    Ok(Artifact::owned(
        ".espforge/manifest.json",
        serde_json::to_string_pretty(&json!({
            "template_version": template_version,
            "files": files,
        }))?,
    ))
}

/// Parse a manifest into `path -> checksum`. Also accepts the older flat
/// format (a bare `path -> checksum` object).
pub fn parse_manifest(src: &str) -> Result<BTreeMap<String, String>> {
    let v: serde_json::Value = serde_json::from_str(src)?;
    let obj = v
        .get("files")
        .and_then(|f| f.as_object())
        .or_else(|| v.as_object());
    Ok(obj
        .into_iter()
        .flatten()
        .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
        .collect())
}

/// FNV-1a 64-bit: dependency-free, enough to detect accidental edits.
pub(crate) fn checksum(content: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in content.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_records_version_and_owned_files_only() {
        let arts = vec![
            Artifact::owned("a.rs", "x"),
            Artifact::seed_once("b.toml", "y"),
        ];
        let m = emit_manifest(&arts, "9.9.9").unwrap();
        assert_eq!(m.path, ".espforge/manifest.json");
        let v: serde_json::Value = serde_json::from_str(&m.content).unwrap();
        assert_eq!(v["template_version"], "9.9.9");
        assert!(v["files"].get("a.rs").is_some());
        assert!(v["files"].get("b.toml").is_none());
    }

    #[test]
    fn parses_both_formats() {
        let new = parse_manifest(r#"{"template_version":"1","files":{"a":"1"}}"#).unwrap();
        assert_eq!(new.get("a").map(String::as_str), Some("1"));
        assert!(!new.contains_key("template_version"));
        let old = parse_manifest(r#"{"a":"1"}"#).unwrap();
        assert_eq!(old.get("a").map(String::as_str), Some("1"));
    }
}
