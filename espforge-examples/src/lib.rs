//! Embedded ESPF examples for the `espforge` binary, exposed as a plumbing
//! catalog.
//!
//! This crate is Layer 0 of the layering described in `cli.md`: it describes
//! and copies the embedded example tree, nothing else. No UI, no stdout, no
//! prompts — presentation lives in the `espforge` binary.
//!
//! Principles:
//! - every example is addressed as `<category>/<name>` via [`find`];
//! - [`materialize`] writes an example verbatim to disk and never clobbers;
//! - plumbing functions take `&Path` in and return data / results out.

use std::ffi::OsStr;
use std::path::Path;
use std::sync::LazyLock;

use include_dir::{include_dir, Dir};

/// The example tree, embedded at compile time from `src/examples`.
pub static EXAMPLES_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/src/examples");

/// Name of the ESPF source file inside each example directory.
const EXAMPLE_FILE: &str = "example.espf.txt";

/// A file inside an example, with its path relative to the example directory.
pub struct ExampleFile {
    /// Relative path, e.g. `app/rust/app.rs` or `example.espf.txt`.
    pub path: String,
    /// Verbatim embedded contents (examples may contain binaries, e.g. wasm).
    pub contents: &'static [u8],
}

/// A top-level example category, e.g. `01.Basics`.
pub struct Category {
    /// Category directory name, e.g. `01.Basics`.
    pub name: String,
    /// Examples directly under this category, sorted by name.
    pub examples: Vec<Example>,
}

/// A single example (a directory containing `example.espf.txt`).
pub struct Example {
    /// Example directory name, e.g. `helloworld`.
    pub name: String,
    /// Human-facing title taken from the banner at the top of the example
    /// file, when one is present (e.g. `HELLO WORLD EXAMPLE`).
    pub title: Option<String>,
    /// Full contents of the example's `example.espf.txt`.
    pub content: String,
    /// Every embedded file of the example, including `example.espf.txt` itself.
    pub files: Vec<ExampleFile>,
}

static CATALOG: LazyLock<Vec<Category>> = LazyLock::new(|| {
    let mut categories: Vec<Category> = EXAMPLES_DIR.dirs().filter_map(category_from_dir).collect();
    categories.sort_by(|a, b| a.name.cmp(&b.name));
    for category in &mut categories {
        category.examples.sort_by(|a, b| a.name.cmp(&b.name));
    }
    categories
});

/// All categories that contain at least one example, sorted by name.
pub fn categories() -> &'static [Category] {
    &CATALOG
}

/// Find an example by `category/name`, e.g. `01.Basics/button`.
pub fn find(example_path: &str) -> Option<&'static Example> {
    let (category_name, example_name) = example_path.split_once('/')?;
    let category = categories().iter().find(|c| c.name == category_name)?;
    category.examples.iter().find(|e| e.name == example_name)
}

/// Plumbing listing of the catalog: one `category/name<TAB>title` line per
/// example, filtered to a single category when given. Plain, script-friendly
/// text; the caller owns printing.
///
/// Errors explain why there is nothing to list (empty tree, unknown category).
pub fn list(category: Option<&str>) -> Result<Vec<String>, String> {
    if categories().is_empty() {
        return Err("no examples are embedded in this build".to_string());
    }
    let selected = match category {
        Some(name) => match categories().iter().find(|c| c.name == name) {
            Some(c) => std::slice::from_ref(c),
            None => {
                let available: Vec<&str> = categories().iter().map(|c| c.name.as_str()).collect();
                return Err(format!(
                    "unknown category '{name}' (available: {})",
                    available.join(", ")
                ));
            }
        },
        None => categories(),
    };
    Ok(selected
        .iter()
        .flat_map(|c| {
            c.examples
                .iter()
                .map(move |e| {
                    format!("{}/{}\t{}", c.name, e.name, e.title.clone().unwrap_or_default())
                })
        })
        .collect())
}

/// Write an example's embedded files verbatim under `dest`.
///
/// Never clobbers: if any target file already exists (or `dest` is an existing
/// file), nothing is written and the error names the path. On success returns
/// the absolute path of every file written.
pub fn materialize(example: &Example, dest: &Path) -> std::io::Result<Vec<std::path::PathBuf>> {
    use std::io;
    use std::path::PathBuf;

    if dest.is_file() {
        return Err(io::Error::other(format!(
            "cannot write '{}': not a directory",
            dest.display()
        )));
    }
    let targets: Vec<PathBuf> = example.files.iter().map(|f| dest.join(&f.path)).collect();
    for target in &targets {
        if target.exists() {
            return Err(io::Error::other(format!(
                "refusing to overwrite '{}' (already exists)",
                target.display()
            )));
        }
    }
    std::fs::create_dir_all(dest)?;
    let mut written = Vec::with_capacity(targets.len());
    for ExampleFile { path, contents } in &example.files {
        let target = dest.join(path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&target, contents)?;
        written.push(target);
    }
    Ok(written)
}

fn category_from_dir(cat_dir: &'static Dir<'static>) -> Option<Category> {
    let examples: Vec<Example> = cat_dir.dirs().filter_map(example_from_dir).collect();
    (!examples.is_empty()).then_some(Category {
        name: path_segment(cat_dir.path()),
        examples,
    })
}

fn example_from_dir(ex_dir: &'static Dir<'static>) -> Option<Example> {
    let espf = ex_dir
        .files()
        .find(|f| f.path().file_name() == Some(OsStr::new(EXAMPLE_FILE)))?;
    let content = espf.contents_utf8()?.to_string();
    let title = extract_title(&content);
    let mut files = Vec::new();
    collect_files(ex_dir, String::new(), &mut files);
    Some(Example {
        name: path_segment(ex_dir.path()),
        title,
        content,
        files,
    })
}

/// Recursively collect an example's files with paths relative to the example
/// directory, composing names level by level (independent of `include_dir`'s
/// path-prefix choices).
fn collect_files(dir: &Dir<'static>, prefix: String, out: &mut Vec<ExampleFile>) {
    for file in dir.files() {
        let name = path_segment(file.path());
        let rel = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        out.push(ExampleFile {
            path: rel,
            contents: file.contents(),
        });
    }
    for sub in dir.dirs() {
        let name = path_segment(sub.path());
        let rel = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        collect_files(sub, rel, out);
    }
}

/// Last segment of an embedded path as a String ("" when it has none).
fn path_segment(path: &Path) -> String {
    path.file_name()
        .and_then(OsStr::to_str)
        .unwrap_or_default()
        .to_string()
}

/// Pull the human title out of the example ADF: the text between the opening
/// `====` banner and the closing one, trimmed. Returns nothing when the
/// top-of-file layout is different.
fn extract_title(content: &str) -> Option<String> {
    let mut lines = content.lines().skip_while(|l| l.trim().is_empty());
    match lines.next()? {
        // File opens with a `====` banner: the next non-banner line is the title.
        first if is_banner(first) => lines
            .find(|l| !is_banner(l) && !l.trim().is_empty())
            .map(|l| l.trim().to_string()),
        first => Some(first.trim().to_string()),
    }
}

fn is_banner(line: &str) -> bool {
    !line.is_empty() && line.chars().all(|c| c == '=')
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Unique scratch directory per `tag`, so tests can run in parallel.
    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "espforge-examples-test-{}-{tag}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn examples_embed_and_parse() {
        let categories = categories();
        assert!(!categories.is_empty());
        let basics = &categories[0];
        assert_eq!(basics.name, "01.Basics");
        // Examples are documented as sorted by name, so `blink` comes first.
        assert!(basics.examples.windows(2).all(|w| w[0].name <= w[1].name));
        let helloworld = basics
            .examples
            .iter()
            .find(|e| e.name == "helloworld")
            .expect("01.Basics/helloworld must be embedded");
        assert_eq!(helloworld.title.as_deref(), Some("HELLO WORLD EXAMPLE"));
        assert!(helloworld.content.contains("# espforge"));
    }

    #[test]
    fn empty_categories_are_dropped() {
        let categories = categories();
        assert!(categories.iter().all(|c| !c.examples.is_empty()));
        let digital = categories
            .iter()
            .find(|c| c.name == "02.Digital")
            .expect("02.Digital examples must be embedded");
        assert_eq!(digital.examples.len(), 2);
    }

    #[test]
    fn find_addresses_examples() {
        let button = find("02.Digital/button").expect("02.Digital/button must exist");
        assert_eq!(button.title.as_deref(), Some("BUTTON EXAMPLE"));
        assert!(find("02.Digital/no-such-example").is_none());
        assert!(find("no-such-category/button").is_none());
        assert!(find("no-slash").is_none());
    }

    #[test]
    fn example_records_every_file() {
        let button = find("02.Digital/button").expect("button must exist");
        let paths: Vec<&str> = button.files.iter().map(|f| f.path.as_str()).collect();
        for expected in ["example.espf.txt", "app/rust/app.rs", "diagram.json"] {
            assert!(paths.contains(&expected), "missing {expected} in {paths:?}");
        }
        let espf = button
            .files
            .iter()
            .find(|f| f.path == "example.espf.txt")
            .expect("espf must be among the files");
        let text = std::str::from_utf8(espf.contents).expect("espf must be utf8");
        assert_eq!(text, button.content);
    }

    #[test]
    fn nested_and_binary_files_are_kept() {
        let i2c = find("04.Communication/i2c_example").expect("i2c_example must exist");
        let zig = i2c
            .files
            .iter()
            .find(|f| f.path == "chip/chip.zig")
            .expect("nested chip/chip.zig must be kept");
        assert!(std::str::from_utf8(zig.contents).is_ok());
        let wasm = i2c
            .files
            .iter()
            .find(|f| f.path == "chip.wasm")
            .expect("binary chip.wasm must be kept");
        assert!(!wasm.contents.is_empty());
        assert!(
            wasm.contents.starts_with(b"\0asm"),
            "wasm payload must be copied verbatim (magic header missing)"
        );
    }

    #[test]
    fn materialize_writes_verbatim_tree() {
        let button = find("02.Digital/button").expect("button must exist");
        let dest = scratch("happy");
        let written = materialize(button, &dest).expect("materialize must succeed");
        assert_eq!(written.len(), button.files.len());
        assert_eq!(
            std::fs::read_to_string(dest.join("example.espf.txt")).expect("espf written"),
            button.content
        );
        let nested = dest.join("app").join("rust").join("app.rs");
        let embedded_app = button
            .files
            .iter()
            .find(|f| f.path == "app/rust/app.rs")
            .expect("app.rs embedded");
        assert_eq!(
            std::fs::read(&nested).expect("app.rs readable"),
            embedded_app.contents
        );
        // Every recorded file landed at its relative path.
        for file in &button.files {
            assert!(dest.join(&file.path).is_file(), "{} missing", file.path);
        }
        let _ = std::fs::remove_dir_all(&dest);
    }

    #[test]
    fn materialize_never_clobbers() {
        let button = find("02.Digital/button").expect("button must exist");
        let dest = scratch("clobber");
        materialize(button, &dest).expect("first materialize must succeed");
        let err = materialize(button, &dest)
            .expect_err("second materialize into the same dest must fail");
        assert!(
            err.to_string().contains("refusing to overwrite"),
            "unexpected error: {err}"
        );
        let _ = std::fs::remove_dir_all(&dest);
    }

    #[test]
    fn list_is_machine_readable() {
        let all = list(None).expect("listing the full tree must work");
        assert!(!all.is_empty());
        let filtered = list(Some("02.Digital")).expect("02.Digital must list");
        assert_eq!(filtered.len(), 2);
        assert!(filtered.iter().all(|l| l.starts_with("02.Digital/")));
        assert!(filtered.iter().all(|l| l.split_once('\t').is_some()));
        let err = list(Some("no-such-category")).expect_err("unknown category must fail");
        assert!(err.contains("unknown category"), "unexpected: {err}");
        assert!(err.contains("01.Basics"), "should name available categories");
    }

    #[test]
    fn every_embedded_example_materializes() {
        for category in categories() {
            for example in &category.examples {
                let dest = scratch(&format!("sweep-{}", example.name));
                let written = materialize(example, &dest).unwrap_or_else(|e| {
                    panic!("materialize {}/{} failed: {e}", category.name, example.name)
                });
                assert_eq!(written.len(), example.files.len());
                let _ = std::fs::remove_dir_all(&dest);
            }
        }
    }
}
