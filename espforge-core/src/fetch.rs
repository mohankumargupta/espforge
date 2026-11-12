//! Host-side fetching for `espforge driver add`.
//!
//! Generation never comes through here: `check`, `gen` and `build` read the
//! cache only. This module owns the three network-ish sources §3 names — an
//! HTTPS archive, a pinned Git revision and an exact registry index entry —
//! plus the extraction and hashing rules that keep a package inside its own
//! cache directory.
//!
//! Local paths are accepted for tests and for `--allow-local-source`, never by
//! default: `file://` and `http://` must be opted into explicitly.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use serde::Deserialize;
use sha2::{Digest, Sha256};

/// The largest archive `driver add` will download.
pub const DOWNLOAD_CAP: u64 = 32 * 1024 * 1024;
/// The largest tree it will extract.
pub const EXTRACT_CAP: u64 = 64 * 1024 * 1024;
/// The most entries it will extract.
pub const EXTRACT_ENTRIES: usize = 4096;

/// Where a driver package comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Spec {
    /// A `.kdl` file or a directory, read in place.
    Path(PathBuf),
    /// A gzipped tar archive, optionally pinned by SHA-256.
    Archive { url: String, sha256: Option<String> },
    /// A repository at one revision.
    Git { url: String, rev: String },
    /// `<tag>@<version>` in a static index.
    Registry {
        index: String,
        tag: String,
        version: String,
    },
}

impl Spec {
    pub fn kind(&self) -> &'static str {
        match self {
            Spec::Path(_) => "local",
            Spec::Archive { .. } => "https",
            Spec::Git { .. } => "git",
            Spec::Registry { .. } => "registry",
        }
    }
}

/// Parse `driver add`'s argument.
pub fn parse_source(
    text: &str,
    rev: Option<&str>,
    registry: Option<&str>,
    allow_local: bool,
) -> Result<Spec, String> {
    if let Some(index) = registry {
        let Some((tag, version)) = text.split_once('@') else {
            return Err(format!(
                "`{text}` is not `<tag>@<version>`; a registry add needs an exact version"
            ));
        };
        if tag.is_empty() || version.is_empty() {
            return Err(format!(
                "`{text}` is not `<tag>@<version>`; a registry add needs an exact version"
            ));
        }
        return Ok(Spec::Registry {
            index: index.to_string(),
            tag: tag.to_string(),
            version: version.to_string(),
        });
    }
    if let Some(rest) = text.strip_prefix("git+") {
        let (url, revision) = match rest.split_once('#') {
            Some((url, revision)) => (url, revision),
            None => (rest, ""),
        };
        let revision = rev.filter(|rev| !rev.is_empty()).unwrap_or(revision);
        if revision.is_empty() {
            return Err(format!(
                "`{text}` names no revision; write `git+https://…#<rev>` or pass `--rev`"
            ));
        }
        check_url(url, allow_local)?;
        return Ok(Spec::Git {
            url: url.to_string(),
            rev: revision.to_string(),
        });
    }
    if text.starts_with("https://") || text.starts_with("http://") || text.starts_with("file://") {
        check_url(text, allow_local)?;
        return Ok(Spec::Archive {
            url: text.to_string(),
            sha256: None,
        });
    }
    Ok(Spec::Path(PathBuf::from(text)))
}

fn check_url(url: &str, allow_local: bool) -> Result<(), String> {
    if url.starts_with("https://") {
        return Ok(());
    }
    if allow_local && (url.starts_with("http://") || url.starts_with("file://")) {
        return Ok(());
    }
    Err(format!(
        "`{url}` is not an HTTPS URL\n   fix: use https://, or pass --allow-local-source for a test fixture"
    ))
}

/// Download an HTTPS (or opted-in local) URL, capped at [`DOWNLOAD_CAP`].
pub fn download(url: &str) -> Result<Vec<u8>, String> {
    if let Some(path) = url.strip_prefix("file://") {
        let bytes = std::fs::read(path)
            .map_err(|error| format!("DRIVER_FETCH_FAILED: cannot read '{path}': {error}"))?;
        if bytes.len() as u64 > DOWNLOAD_CAP {
            return Err(format!(
                "DRIVER_FETCH_FAILED: '{url}' is larger than {} MiB",
                DOWNLOAD_CAP / (1024 * 1024)
            ));
        }
        return Ok(bytes);
    }
    let response = ureq::get(url)
        .call()
        .map_err(|error| format!("DRIVER_FETCH_FAILED: cannot fetch '{url}': {error}"))?;
    let mut bytes = Vec::new();
    response
        .into_reader()
        .take(DOWNLOAD_CAP + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("DRIVER_FETCH_FAILED: cannot read '{url}': {error}"))?;
    if bytes.len() as u64 > DOWNLOAD_CAP {
        return Err(format!(
            "DRIVER_FETCH_FAILED: '{url}' is larger than {} MiB",
            DOWNLOAD_CAP / (1024 * 1024)
        ));
    }
    Ok(bytes)
}

/// Extract a gzipped tar into `dest`, refusing traversal, links and oversize.
pub fn extract_archive(bytes: &[u8], dest: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dest)
        .map_err(|error| format!("cannot create '{}': {error}", dest.display()))?;
    let reader: Box<dyn Read> = if bytes.starts_with(&[0x1f, 0x8b]) {
        Box::new(flate2::read::GzDecoder::new(std::io::Cursor::new(bytes)))
    } else {
        Box::new(std::io::Cursor::new(bytes))
    };
    let mut archive = tar::Archive::new(reader);
    let entries = archive
        .entries()
        .map_err(|error| format!("DRIVER_ARCHIVE_UNSAFE: {error}"))?;
    let mut count = 0usize;
    let mut total = 0u64;
    for entry in entries {
        count += 1;
        if count > EXTRACT_ENTRIES {
            return Err(format!(
                "DRIVER_ARCHIVE_UNSAFE: the archive has more than {EXTRACT_ENTRIES} entries"
            ));
        }
        let mut entry = entry.map_err(|error| format!("DRIVER_ARCHIVE_UNSAFE: {error}"))?;
        let kind = entry.header().entry_type();
        if !kind.is_file() && !kind.is_dir() {
            return Err(format!(
                "DRIVER_ARCHIVE_UNSAFE: refusing a link or device entry at '{}'",
                entry
                    .path()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|_| "?".into())
            ));
        }
        let path = entry
            .path()
            .map_err(|error| format!("DRIVER_ARCHIVE_UNSAFE: {error}"))?;
        if path.is_absolute()
            || path
                .components()
                .any(|component| !matches!(component, Component::Normal(_) | Component::CurDir))
        {
            return Err(format!(
                "DRIVER_ARCHIVE_UNSAFE: '{}' escapes the package",
                path.display()
            ));
        }
        let target = dest.join(&path);
        if kind.is_dir() {
            std::fs::create_dir_all(&target)
                .map_err(|error| format!("cannot create '{}': {error}", target.display()))?;
            continue;
        }
        let size = entry.header().size().unwrap_or(0);
        total = total.saturating_add(size);
        if total > EXTRACT_CAP {
            return Err(format!(
                "DRIVER_ARCHIVE_UNSAFE: the archive expands past {} MiB",
                EXTRACT_CAP / (1024 * 1024)
            ));
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("cannot create '{}': {error}", parent.display()))?;
        }
        entry.unpack(&target).map_err(|error| {
            format!(
                "DRIVER_ARCHIVE_UNSAFE: cannot extract '{}': {error}",
                target.display()
            )
        })?;
    }
    Ok(())
}

/// Clone/fetch `url` at `rev` into `dest`, returning the full commit SHA.
///
/// No submodules, no hooks, no terminal prompt. Only `https://` (and, with
/// `--allow-local-source`, `file://`) reach here.
pub fn git_checkout(url: &str, rev: &str, dest: &Path) -> Result<String, String> {
    std::fs::create_dir_all(dest)
        .map_err(|error| format!("cannot create '{}': {error}", dest.display()))?;
    let git = |args: &[&str]| -> Result<String, String> {
        let output = Command::new("git")
            .args(["-c", "core.hooksPath=/dev/null", "--no-pager"])
            .args(args)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .output()
            .map_err(|error| {
                format!("DRIVER_FETCH_FAILED: cannot run git: {error}\n   fix: install git")
            })?;
        if !output.status.success() {
            return Err(format!(
                "DRIVER_FETCH_FAILED: git {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    };
    let dest_text = dest.to_string_lossy().to_string();
    git(&["init", "--quiet", &dest_text])?;
    git(&["-C", &dest_text, "remote", "add", "origin", url])?;
    git(&[
        "-C",
        &dest_text,
        "fetch",
        "--quiet",
        "--depth",
        "1",
        "--no-recurse-submodules",
        "origin",
        rev,
    ])?;
    git(&[
        "-C",
        &dest_text,
        "checkout",
        "--quiet",
        "--detach",
        "FETCH_HEAD",
    ])?;
    let commit = git(&["-C", &dest_text, "rev-parse", "HEAD"])?;
    let _ = std::fs::remove_dir_all(dest.join(".git"));
    Ok(commit)
}

#[derive(Deserialize)]
struct IndexDocument {
    schema: u32,
    drivers: BTreeMap<String, Vec<IndexEntry>>,
}

#[derive(Deserialize)]
struct IndexEntry {
    version: String,
    url: String,
    #[serde(default)]
    sha256: Option<String>,
}

/// Look up `<tag>@<version>` in a static index, returning its URL and hash.
pub fn registry_entry(
    index: &str,
    tag: &str,
    version: &str,
) -> Result<(String, Option<String>), String> {
    let bytes = download(index).map_err(|error| format!("DRIVER_INDEX_INVALID: {error}"))?;
    let document: IndexDocument = serde_json::from_slice(&bytes).map_err(|error| {
        format!("DRIVER_INDEX_INVALID: '{index}' is not a valid index: {error}")
    })?;
    if document.schema != 1 {
        return Err(format!(
            "DRIVER_INDEX_INVALID: '{index}' declares schema {}, expected 1",
            document.schema
        ));
    }
    let entries = document
        .drivers
        .get(tag)
        .ok_or_else(|| format!("DRIVER_INDEX_INVALID: the index has no driver `<{tag}>`"))?;
    let entry = entries
        .iter()
        .find(|entry| entry.version == version)
        .ok_or_else(|| {
            format!("DRIVER_INDEX_INVALID: `<{tag}>` has no version {version} in the index")
        })?;
    Ok((entry.url.clone(), entry.sha256.clone()))
}

/// The SHA-256 of a package's files, over sorted relative paths and contents.
pub fn payload_hash(root: &Path) -> Result<String, String> {
    let mut files = Vec::new();
    collect_files(root, root, &mut files)?;
    files.sort();
    let mut hasher = Sha256::new();
    for (relative, path) in files {
        // The manifest and cache metadata are hashed separately: the payload
        // hash names the crate files alone, and `source.json` is written after
        // the hash is taken.
        if relative == "driver.kdl" || relative == "source.json" {
            continue;
        }
        hasher.update(relative.as_bytes());
        hasher.update([0]);
        let bytes = std::fs::read(&path)
            .map_err(|error| format!("cannot read '{}': {error}", path.display()))?;
        hasher.update(&bytes);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Copy every file under `src` into `dest`, skipping `.git`.
pub fn copy_tree(src: &Path, dest: &Path) -> Result<(), String> {
    let mut files = Vec::new();
    collect_files(src, src, &mut files)?;
    for (relative, path) in files {
        let target = dest.join(&relative);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("cannot create '{}': {error}", parent.display()))?;
        }
        std::fs::copy(&path, &target).map_err(|error| {
            format!(
                "cannot copy '{}' to '{}': {error}",
                path.display(),
                target.display()
            )
        })?;
    }
    Ok(())
}

fn collect_files(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) -> Result<(), String> {
    let mut entries = std::fs::read_dir(dir)
        .map_err(|error| format!("cannot read '{}': {error}", dir.display()))?
        .collect::<std::io::Result<Vec<_>>>()
        .map_err(|error| error.to_string())?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name();
        if name == ".git" {
            continue;
        }
        if path.is_dir() {
            collect_files(root, &path, out)?;
        } else if path.is_file() {
            let relative = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            out.push((relative, path));
        }
    }
    Ok(())
}

/// Find `driver.kdl` at the root or one directory down.
pub fn find_manifest(root: &Path) -> Result<PathBuf, String> {
    let direct = root.join("driver.kdl");
    if direct.is_file() {
        return Ok(direct);
    }
    let mut found = Vec::new();
    let entries = std::fs::read_dir(root)
        .map_err(|error| format!("cannot read '{}': {error}", root.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let candidate = path.join("driver.kdl");
            if candidate.is_file() {
                found.push(candidate);
            }
        }
    }
    match found.len() {
        1 => Ok(found.remove(0)),
        0 => Err(format!(
            "DRIVER_FETCH_FAILED: '{}' contains no driver.kdl",
            root.display()
        )),
        _ => Err(format!(
            "DRIVER_FETCH_FAILED: '{}' contains several driver.kdl files",
            root.display()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_path_is_local_and_a_url_is_an_archive() {
        assert!(matches!(
            parse_source("drivers/x.kdl", None, None, false).unwrap(),
            Spec::Path(_)
        ));
        assert!(matches!(
            parse_source("https://example.invalid/x.tar.gz", None, None, false).unwrap(),
            Spec::Archive { .. }
        ));
        assert!(parse_source("http://example.invalid/x", None, None, false).is_err());
        assert!(parse_source("http://example.invalid/x", None, None, true).is_ok());
    }

    #[test]
    fn a_git_source_needs_a_revision() {
        assert!(parse_source("git+https://example.invalid/x", None, None, false).is_err());
        assert!(matches!(
            parse_source("git+https://example.invalid/x#abc", None, None, false).unwrap(),
            Spec::Git { .. }
        ));
    }

    #[test]
    fn a_registry_add_is_exact() {
        assert!(matches!(
            parse_source("fx@0.1.0", None, Some("https://index"), false).unwrap(),
            Spec::Registry { .. }
        ));
        assert!(parse_source("fx", None, Some("https://index"), false).is_err());
    }

    #[test]
    fn a_traversing_archive_is_refused() {
        // `tar::Builder` refuses `..` before writing, so build the header by
        // hand: this is exactly the archive a hostile server could send.
        let mut header = tar::Header::new_gnu();
        header.set_size(1);
        header.set_mode(0o644);
        header.set_entry_type(tar::EntryType::Regular);
        header.as_mut_bytes()[..9].copy_from_slice(b"../escape");
        header.set_cksum();
        let mut bytes = Vec::new();
        bytes.extend_from_slice(header.as_bytes());
        bytes.extend_from_slice(b"x");
        bytes.resize(2048, 0);
        let dir = tempfile::tempdir().unwrap();
        let error = extract_archive(&bytes, dir.path()).unwrap_err();
        assert!(error.contains("DRIVER_ARCHIVE_UNSAFE"), "{error}");
    }

    #[test]
    fn a_payload_hash_is_order_independent() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("b"), b"b").unwrap();
        std::fs::write(dir.path().join("a"), b"a").unwrap();
        let first = payload_hash(dir.path()).unwrap();
        let dir2 = tempfile::tempdir().unwrap();
        std::fs::write(dir2.path().join("a"), b"a").unwrap();
        std::fs::write(dir2.path().join("b"), b"b").unwrap();
        assert_eq!(first, payload_hash(dir2.path()).unwrap());
    }
}
