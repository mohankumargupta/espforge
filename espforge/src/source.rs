//! Where an ESPF spec comes from: a file, an embedded example, or the single
//! `.espf` in the current directory — plus every other read of the user's
//! directory (`build` file collection: Wokwi overlays, app logic, runtime).
//!
//! Extracted from `commands::pipeline` (step 1c) and `commands::build`
//! (step 3c): source discovery + file collection. No validation here;
//! `check`/`build` resolve a [`Source`] first, then run
//! `pipeline::validate` on `source.text`.
//!
//! The query path going forward: `resolve_source` -> `validate` -> `hardware::lower`.

use anyhow::{Context, Result};
use espforge_emit::RuntimeSource;
use std::path::{Path, PathBuf};

pub struct Source {
    /// Name used in diagnostics: the path, or `embedded:<category>/<name>`.
    pub name: String,
    pub text: String,
    /// Directory of the `.espf` (None for embedded examples).
    pub dir: Option<PathBuf>,
    pub example: Option<&'static espforge_examples::Example>,
}

pub fn from_file(file: &Path) -> Result<Source, String> {
    let text = std::fs::read_to_string(file)
        .map_err(|e| format!("cannot read '{}': {e}", file.display()))?;
    let dir = file
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    Ok(Source {
        name: file.to_string_lossy().into_owned(),
        text,
        dir: Some(dir),
        example: None,
    })
}

/// The single `*.espf` / `*.espf.txt` file in `dir`.
pub fn find_project(dir: &Path) -> Result<PathBuf, String> {
    let entries =
        std::fs::read_dir(dir).map_err(|e| format!("cannot read '{}': {e}", dir.display()))?;
    let mut names: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n.ends_with(".espf") || n.ends_with(".espf.txt"))
        .collect();
    names.sort();
    let at = |name: &str| {
        if dir == Path::new(".") {
            PathBuf::from(name)
        } else {
            dir.join(name)
        }
    };
    match names.len() {
        0 => Err(format!(
            "no .espf file in '{}'; give a FILE or --example <category/name>",
            dir.display()
        )),
        1 => Ok(at(&names[0])),
        _ => Err(format!(
            "several .espf files in '{}' ({}); give one explicitly",
            dir.display(),
            names.join(", ")
        )),
    }
}

/// FILE, `--example`, or (neither) the single `.espf` in the current directory.
pub fn resolve_source(file: Option<&Path>, example: Option<&str>) -> Result<Source, String> {
    match (file, example) {
        (Some(_), Some(_)) => Err("give either FILE or --example, not both".to_string()),
        (Some(f), None) => from_file(f),
        (None, None) => from_file(&find_project(Path::new("."))?),
        (None, Some(path)) => {
            let ex = espforge_examples::find(path).ok_or_else(|| {
                format!(
                    "no example '{path}'; run `espforge examples list` to see what is available"
                )
            })?;
            Ok(Source {
                name: format!("embedded:{path}"),
                text: ex.content.clone(),
                dir: None,
                example: Some(ex),
            })
        }
    }
}

// ------------------------------------------------------- file collection
//
// Everything below reads the user's directory (never writes): Wokwi overlay
// files, app logic, and the runtime source. Moved verbatim from
// `commands::build` (step 3c); `build.rs::execute` is now the ~10-line
// pipeline it always was: resolve -> validate -> generate -> write_all.

/// Files next to a `.espf` that customise the generated Wokwi setup.
const OVERLAY_FILES: [&str; 4] = ["diagram.json", "wokwi.toml", "chip.json", "chip.wasm"];

fn collect_dir(dir: &Path, prefix: &str, out: &mut Vec<(String, Vec<u8>)>) -> Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    let mut entries = std::fs::read_dir(dir)?.collect::<std::io::Result<Vec<_>>>()?;
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let rel = format!("{prefix}/{}", e.file_name().to_string_lossy());
        let path = e.path();
        if path.is_dir() {
            collect_dir(&path, &rel, out)?;
        } else {
            out.push((rel, std::fs::read(&path)?));
        }
    }
    Ok(())
}

/// The example files the template render may layer on top: from the embedded
/// example, or from the folder the `.espf` lives in (a materialized example).
pub fn overlay_files(src: &Source) -> Result<Vec<(String, Vec<u8>)>> {
    if let Some(ex) = src.example {
        return Ok(ex
            .files
            .iter()
            .map(|f| (f.path.clone(), f.contents.to_vec()))
            .collect());
    }
    let dir = src.dir.as_ref().context("a file source has a directory")?;
    let mut out = Vec::new();
    for name in OVERLAY_FILES {
        let p = dir.join(name);
        if p.is_file() {
            out.push((
                name.to_string(),
                std::fs::read(&p).with_context(|| format!("cannot read {}", p.display()))?,
            ));
        }
    }
    collect_dir(&dir.join("chip"), "chip", &mut out)?;
    Ok(out)
}

/// App logic lives in `src/app.rs` first, `app/rust/app.rs` second.
pub fn find_app(dir: &Path) -> Option<PathBuf> {
    let preferred = dir.join("src").join("app.rs");
    if preferred.is_file() {
        return Some(preferred);
    }
    let example_layout = dir.join("app").join("rust").join("app.rs");
    example_layout.is_file().then_some(example_layout)
}

/// The user's app-logic bytes, copied verbatim into the generated project.
///
/// `Err` is a user-facing message (the caller reports it and stops with a
/// user error). Hard IO failures surface as their `to_string()`; the
/// previous `Failure::Internal` distinction for those rare paths is gone —
/// everything unreadable here is reported the same way.
pub fn app_logic(src: &Source) -> Result<Vec<u8>, String> {
    if let Some(ex) = src.example {
        return ex
            .files
            .iter()
            .find(|f| f.path == "app/rust/app.rs")
            .map(|f| f.contents.to_vec())
            .ok_or_else(|| "this example has no app/rust/app.rs".to_string());
    }
    let dir = src.dir.as_ref().expect("a file source has a directory");
    let found = find_app(dir).ok_or_else(|| {
        format!(
            "missing {} (or {}): your app logic (setup/forever)",
            dir.join("src").join("app.rs").display(),
            dir.join("app").join("rust").join("app.rs").display()
        )
    })?;
    std::fs::read(&found).map_err(|e| e.to_string())
}

/// Which `espforge-runtime` the generated `Cargo.toml` references: the
/// published crate, or `--runtime-path` / `ESPFORGE_RUNTIME_PATH`.
///
/// `Err` is a user-facing message (the caller reports it and stops).
pub fn runtime_source(runtime_path: Option<PathBuf>) -> Result<RuntimeSource, String> {
    let path = runtime_path.or_else(|| std::env::var_os("ESPFORGE_RUNTIME_PATH").map(PathBuf::from));
    match path {
        Some(p) => {
            let abs = std::path::absolute(&p).map_err(|e| e.to_string())?;
            if !abs.is_dir() {
                return Err(format!(
                    "runtime path '{}' is not a directory",
                    p.display()
                ));
            }
            Ok(RuntimeSource::Path(abs.to_string_lossy().into_owned()))
        }
        None => Ok(RuntimeSource::Published),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn find_project_wants_exactly_one() {
        let dir = tempfile::tempdir().unwrap();
        assert!(find_project(dir.path()).is_err());
        std::fs::write(dir.path().join("a.espf"), "").unwrap();
        std::fs::write(dir.path().join("notes.txt"), "").unwrap();
        assert_eq!(find_project(dir.path()).unwrap(), dir.path().join("a.espf"));
        std::fs::write(dir.path().join("b.espf.txt"), "").unwrap();
        let err = find_project(dir.path()).unwrap_err();
        assert!(
            err.contains("a.espf") && err.contains("b.espf.txt"),
            "{err}"
        );
    }

    #[test]
    fn file_and_example_together_are_rejected() {
        assert!(resolve_source(Some(Path::new("x.espf")), Some("01.Basics/helloworld")).is_err());
        assert!(resolve_source(None, Some("no/such")).is_err());
    }

    #[test]
    fn find_app_prefers_src_app_rs() {
        let dir = tempfile::tempdir().unwrap();
        assert!(find_app(dir.path()).is_none());
        std::fs::create_dir_all(dir.path().join("app/rust")).unwrap();
        std::fs::write(dir.path().join("app/rust/app.rs"), "").unwrap();
        assert!(find_app(dir.path()).unwrap().ends_with("app/rust/app.rs"));
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/app.rs"), "").unwrap();
        assert!(find_app(dir.path()).unwrap().ends_with("src/app.rs"));
    }

    #[test]
    fn materialized_example_folders_overlay_their_wokwi_files() {
        let dir = tempfile::tempdir().unwrap();
        // i2c_scanner has peripherals, so build only its Wokwi overlay logic
        // through overlay_files on a materialized copy.
        let ex = espforge_examples::find("04.Communication/i2c_scanner").unwrap();
        let folder = dir.path().join("ex");
        espforge_examples::materialize(ex, &folder).unwrap();
        let src = Source {
            name: "x".into(),
            text: String::new(),
            dir: Some(folder),
            example: None,
        };
        let files = overlay_files(&src).unwrap();
        let names: Vec<&str> = files.iter().map(|(n, _)| n.as_str()).collect();
        assert!(
            names.contains(&"diagram.json")
                && names.contains(&"chip.json")
                && names.contains(&"chip.wasm"),
            "{names:?}"
        );
        assert!(names.contains(&"chip/chip.zig"), "{names:?}");
        assert!(!names.contains(&"example.espf.txt"));
    }
}
