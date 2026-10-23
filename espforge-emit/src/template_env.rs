//! minijinja environment setup for the embedded project template
//! (`espforge-espgenerate-template`): file listing, environment, rendering.
//!
//! Every file in the template tree is registered by its path, so
//! `{% include ".template/partials/main_blocking.rs" %}` resolves. Files
//! under `.template/` are partials: includable but never emitted themselves.

use anyhow::{Context as _, Result, anyhow};
use espforge_espgenerate_template::TEMPLATE_DIR;
use include_dir::Dir;
use minijinja::{AutoEscape, Environment, UndefinedBehavior, Value};

use crate::metadata::METADATA_FILE;

const PARTIALS_PREFIX: &str = ".template/";

// ------------------------------------------------------------- template files

/// Every embedded file as (`/`-separated path, bytes), paths composed level by
/// level so they do not depend on include_dir's prefix choices.
fn collect_files(dir: &Dir<'static>, prefix: &str, out: &mut Vec<(String, &'static [u8])>) {
    let join = |name: &str| {
        if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}/{name}")
        }
    };
    for f in dir.files() {
        let name = f
            .path()
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        out.push((join(name), f.contents()));
    }
    for d in dir.dirs() {
        let name = d
            .path()
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        collect_files(d, &join(name), out);
    }
}

fn all_files() -> Vec<(String, &'static [u8])> {
    let mut out = Vec::new();
    collect_files(&TEMPLATE_DIR, "", &mut out);
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// Paths of the files that can be emitted (excludes `metadata.toml` and the
/// `.template/` partials), sorted.
pub fn template_paths() -> Vec<String> {
    all_files()
        .into_iter()
        .map(|(p, _)| p)
        .filter(|p| p != METADATA_FILE && !p.starts_with(PARTIALS_PREFIX))
        .collect()
}

/// The minijinja environment: strict about undefined names, no stray blank
/// lines around `{% %}` blocks, no escaping (these are not HTML), and every
/// template file (including partials) registered by path.
pub fn environment() -> Result<Environment<'static>> {
    let mut env = Environment::new();
    env.set_undefined_behavior(UndefinedBehavior::Strict);
    env.set_trim_blocks(true);
    env.set_lstrip_blocks(true);
    env.set_keep_trailing_newline(true);
    // The default callback would JSON-escape values in `*.json` templates.
    env.set_auto_escape_callback(|_| AutoEscape::None);

    for (path, bytes) in all_files() {
        if path == METADATA_FILE {
            continue;
        }
        let text = std::str::from_utf8(bytes)
            .with_context(|| format!("template `{path}` is not valid UTF-8"))?
            .to_string();
        env.add_template_owned(path.clone(), text)
            .map_err(|e| anyhow!("template `{path}`: {e:#}"))?;
    }
    Ok(env)
}

pub fn render_template(env: &Environment, name: &str, ctx: &Value) -> Result<String> {
    env.get_template(name)
        .and_then(|t| t.render(ctx))
        .map_err(|e| anyhow!("rendering `{name}`: {e:#}"))
}
