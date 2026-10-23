//! `metadata.toml` parsing: which template files are emitted, under which
//! names, and with what ownership (`parse_metadata`, `metadata`).
//!
//! ```toml
//! emit = [
//!   { path = "diagram.json", when = "opt.wokwi" },
//! ]
//! ownership = [
//!   { glob = "src/**", own = "owned" },
//! ]
//! ```
//!
//! Files not listed under `emit` are always emitted.

use anyhow::{Context, Result, bail};
use espforge_espgenerate_template::TEMPLATE_DIR;
use espforge_model::Ownership;
use toml_edit::DocumentMut;

pub(crate) const METADATA_FILE: &str = "metadata.toml";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmitEntry {
    /// Output path (without `.tmpl`).
    pub path: String,
    /// minijinja expression; the file is emitted only if it is true.
    pub when: Option<String>,
    /// minijinja template for a different output path.
    pub rename: Option<String>,
    /// Ownership for this file, overriding the glob rules.
    pub own: Option<Ownership>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Metadata {
    pub emit: Vec<EmitEntry>,
    /// (glob, ownership); first match wins; default is `Owned`.
    pub ownership: Vec<(String, Ownership)>,
}

fn parse_own(s: &str) -> Result<Ownership> {
    match s {
        "owned" => Ok(Ownership::Owned),
        "seed-once" => Ok(Ownership::SeedOnce),
        "not-owned" => Ok(Ownership::NotOwned),
        other => bail!("unknown ownership \"{other}\" (expected owned, seed-once or not-owned)"),
    }
}

/// Globs: an exact path, or `dir/**` for everything below `dir`.
fn check_glob(g: &str) -> Result<()> {
    if g.contains('*') && !(g.ends_with("/**") && !g[..g.len() - 3].contains('*')) {
        bail!("unsupported glob \"{g}\" (use an exact path or `dir/**`)");
    }
    Ok(())
}

pub fn glob_match(glob: &str, path: &str) -> bool {
    match glob.strip_suffix("/**") {
        Some(dir) => path.starts_with(&format!("{dir}/")),
        None => glob == path,
    }
}

/// Parse `metadata.toml`:
///
/// ```toml
/// emit = [
///   { path = "diagram.json", when = "opt.wokwi" },
///   { path = ".vscode/tasks.json", when = "opt.vscode", own = "seed-once" },
/// ]
/// ownership = [
///   { glob = "src/**", own = "owned" },
///   { glob = ".cargo/**", own = "seed-once" },
/// ]
/// ```
///
/// Files not listed under `emit` are always emitted.
pub fn parse_metadata(src: &str) -> Result<Metadata> {
    let doc: DocumentMut = src.parse().context("invalid metadata.toml")?;
    let mut md = Metadata::default();

    if let Some(arr) = doc.get("emit").and_then(|i| i.as_array()) {
        for item in arr.iter() {
            let t = item
                .as_inline_table()
                .context("each `emit` entry must be an inline table")?;
            let get = |k: &str| t.get(k).and_then(|v| v.as_str()).map(str::to_string);
            md.emit.push(EmitEntry {
                path: get("path").context("an `emit` entry is missing `path`")?,
                when: get("when"),
                rename: get("as"),
                own: get("own").map(|s| parse_own(&s)).transpose()?,
            });
        }
    }
    if let Some(arr) = doc.get("ownership").and_then(|i| i.as_array()) {
        for item in arr.iter() {
            let t = item
                .as_inline_table()
                .context("each `ownership` entry must be an inline table")?;
            let glob = t
                .get("glob")
                .and_then(|v| v.as_str())
                .context("an `ownership` entry is missing `glob`")?;
            let own = t
                .get("own")
                .and_then(|v| v.as_str())
                .context("an `ownership` entry is missing `own`")?;
            check_glob(glob)?;
            md.ownership.push((glob.to_string(), parse_own(own)?));
        }
    }
    Ok(md)
}

pub fn metadata() -> Result<Metadata> {
    let file = TEMPLATE_DIR
        .get_file(METADATA_FILE)
        .context("the template has no metadata.toml")?;
    parse_metadata(file.contents_utf8().context("metadata.toml is not UTF-8")?)
}

