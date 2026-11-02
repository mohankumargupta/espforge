//! Where an ESPF spec comes from: a file, an embedded example, or the single
//! `.espf` in a directory.
//!
//! Discovery takes the directory it looks in, never `$PWD`: the binary passes
//! `.`, tests pass a scratch directory. App logic and overlay files come from
//! the example (embedded) or from next to the file, including a custom Wokwi
//! chip: `chip.json` and `chip.wasm` always, and the `chip/` Zig sources only
//! when `keep-zig-source` is on.

use anyhow::Result;
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

/// The single `.espf` / `.espf.txt` directly inside `dir`.
///
/// Top-level only: a generated project keeps its own copy of the spec under
/// `build/.espforge/`, which is not something to discover as project source.
pub fn discover_in_dir(dir: &Path) -> Result<Source, String> {
    if !dir.is_dir() {
        return Err(format!("'{}' is not a directory", dir.display()));
    }

    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| format!("cannot read '{}': {e}", dir.display()))?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(".espf") || name.ends_with(".espf.txt"))
        })
        .collect();
    found.sort();

    match found.len() {
        1 => from_file(&found[0]),
        0 => Err(format!(
            "no .espf or .espf.txt file found in '{}'\n   Give a FILE or --example <example>, or run `espforge new <example> <dir>` to start a project.",
            dir.display()
        )),
        _ => {
            let list = found
                .iter()
                .map(|path| {
                    format!(
                        "  - {}",
                        path.file_name().unwrap_or_default().to_string_lossy()
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            Err(format!(
                "multiple ESPF files found in '{}':\n{list}\n   Name the one you want, e.g. `espforge gen {}`.",
                dir.display(),
                found[0].display()
            ))
        }
    }
}

/// FILE, `--example`, or the single `.espf` in `cwd`, never two at once.
///
/// An embedded example may be given as `category/name` or by a unique short
/// name; `cwd` is the directory bare invocations look in.
pub fn resolve_source(
    file: Option<&Path>,
    example: Option<&str>,
    cwd: &Path,
) -> Result<Source, String> {
    match (file, example) {
        (Some(_), Some(_)) => Err("give either FILE or --example, not both".to_string()),
        (Some(path), None) if path.is_dir() => discover_in_dir(path),
        (Some(path), None) => from_file(path),
        (None, None) => discover_in_dir(cwd),
        (None, Some(name)) => {
            let (category, ex) = espforge_examples::resolve_name(name)?;
            Ok(Source {
                name: format!("embedded:{category}/{}", ex.name),
                text: ex.content.clone(),
                dir: None,
                example: Some(ex),
            })
        }
    }
}

/// Files next to a `.espf` that customise the generated Wokwi setup.
const OVERLAY_FILES: [&str; 4] = ["diagram.json", "wokwi.toml", "chip.json", "chip.wasm"];

/// Whether an overlay path is one the render accepts.
fn is_overlay_file(path: &str, keep_zig: bool) -> bool {
    OVERLAY_FILES.contains(&path) || (keep_zig && path.starts_with("chip/"))
}

/// The example files the render may layer on top. `keep_zig` decides whether
/// the optional `chip/` source tree comes along.
pub fn overlay_files(src: &Source, keep_zig: bool) -> Result<Vec<(String, Vec<u8>)>, String> {
    if let Some(ex) = src.example {
        return Ok(ex
            .files
            .iter()
            .filter(|f| is_overlay_file(&f.path, keep_zig))
            .map(|f| (f.path.clone(), f.contents.to_vec()))
            .collect());
    }
    let dir = src
        .dir
        .as_ref()
        .ok_or_else(|| "this source has no directory".to_string())?;
    let mut out = Vec::new();
    for name in OVERLAY_FILES {
        let p = dir.join(name);
        if p.is_file() {
            out.push((
                name.to_string(),
                std::fs::read(&p).map_err(|e| format!("cannot read {}: {e}", p.display()))?,
            ));
        }
    }
    if keep_zig && dir.join("chip").is_dir() {
        read_tree(&dir.join("chip"), "chip", &mut out)?;
    }
    Ok(out)
}

/// Collect every file under `dir`, prefixing its path with `prefix`.
fn read_tree(dir: &Path, prefix: &str, out: &mut Vec<(String, Vec<u8>)>) -> Result<(), String> {
    let mut entries = std::fs::read_dir(dir)
        .map_err(|e| format!("cannot read {}: {e}", dir.display()))?
        .collect::<std::io::Result<Vec<_>>>()
        .map_err(|e| e.to_string())?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        let rel = format!("{prefix}/{name}");
        if path.is_dir() {
            read_tree(&path, &rel, out)?;
        } else {
            out.push((
                rel,
                std::fs::read(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?,
            ));
        }
    }
    Ok(())
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
pub fn app_logic(src: &Source) -> Result<Vec<u8>, String> {
    if let Some(ex) = src.example {
        return ex
            .files
            .iter()
            .find(|f| f.path == "app/rust/app.rs")
            .map(|f| f.contents.to_vec())
            .ok_or_else(|| "this example has no app/rust/app.rs".to_string());
    }
    let dir = src
        .dir
        .as_ref()
        .ok_or_else(|| "this source has no directory to read app logic from".to_string())?;
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
pub fn runtime_source(runtime_path: Option<PathBuf>) -> Result<RuntimeSource, String> {
    let path =
        runtime_path.or_else(|| std::env::var_os("ESPFORGE_RUNTIME_PATH").map(PathBuf::from));
    match path {
        Some(p) => {
            let abs = std::path::absolute(&p).map_err(|e| e.to_string())?;
            if !abs.is_dir() {
                return Err(format!("runtime path '{}' is not a directory", p.display()));
            }
            Ok(RuntimeSource::Path(abs.to_string_lossy().into_owned()))
        }
        None => Ok(RuntimeSource::Published),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    fn write_hello(dir: &Path) -> PathBuf {
        let hello = espforge_examples::find("01.Basics/helloworld").unwrap();
        let file = dir.join("hello.espf");
        std::fs::write(&file, &hello.content).unwrap();
        file
    }

    #[test]
    fn file_and_example_together_are_rejected() {
        let cwd = scratch();
        assert!(
            resolve_source(
                Some(Path::new("x.espf")),
                Some("01.Basics/helloworld"),
                cwd.path()
            )
            .is_err()
        );
    }

    #[test]
    fn an_empty_directory_has_no_source_to_discover() {
        let cwd = scratch();
        let err = resolve_source(None, None, cwd.path()).err().unwrap();
        assert!(err.contains("no .espf"), "{err}");
        assert!(err.contains("espforge new"), "{err}");
    }

    #[test]
    fn discovery_takes_the_single_espf_in_the_directory() {
        let cwd = scratch();
        let file = write_hello(cwd.path());
        let src = resolve_source(None, None, cwd.path()).unwrap();
        assert_eq!(src.name, file.to_string_lossy());
        assert_eq!(src.dir.as_deref(), Some(cwd.path()));

        // A directory argument discovers the same way.
        let src = resolve_source(Some(cwd.path()), None, Path::new(".")).unwrap();
        assert_eq!(src.name, file.to_string_lossy());
    }

    #[test]
    fn several_espf_files_are_named_back_to_the_user() {
        let cwd = scratch();
        write_hello(cwd.path());
        std::fs::write(cwd.path().join("second.espf"), "# espforge\n").unwrap();
        let err = resolve_source(None, None, cwd.path()).err().unwrap();
        assert!(err.contains("multiple ESPF files"), "{err}");
        assert!(err.contains("second.espf"), "{err}");
    }

    #[test]
    fn an_example_source_carries_its_text_and_files() {
        let src = resolve_source(None, Some("01.Basics/helloworld"), Path::new(".")).unwrap();
        assert_eq!(src.name, "embedded:01.Basics/helloworld");
        assert!(src.text.contains("# espforge"));
        assert!(src.dir.is_none());
        assert!(src.example.is_some());
        let overlays = overlay_files(&src, false).unwrap();
        assert!(overlays.is_empty(), "helloworld has no Wokwi overlay file");
        assert!(
            String::from_utf8(app_logic(&src).unwrap())
                .unwrap()
                .contains("Hello World!")
        );
    }

    #[test]
    fn a_short_example_name_resolves_to_its_category_key() {
        let src = resolve_source(None, Some("blink"), Path::new(".")).unwrap();
        assert_eq!(src.name, "embedded:01.Basics/blink");
    }

    #[test]
    fn find_app_prefers_src_app_rs() {
        let dir = scratch();
        assert!(find_app(dir.path()).is_none());
        std::fs::create_dir_all(dir.path().join("app/rust")).unwrap();
        std::fs::write(dir.path().join("app/rust/app.rs"), "").unwrap();
        assert!(find_app(dir.path()).unwrap().ends_with("app/rust/app.rs"));
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/app.rs"), "").unwrap();
        assert!(find_app(dir.path()).unwrap().ends_with("src/app.rs"));
    }

    #[test]
    fn a_runtime_path_must_be_a_directory() {
        let dir = scratch();
        let ok = runtime_source(Some(dir.path().to_path_buf())).unwrap();
        assert!(matches!(ok, RuntimeSource::Path(_)));
        assert!(runtime_source(Some(dir.path().join("nope"))).is_err());
    }
}
