//! Embedded ESPF examples: the catalog, the listing, and materialization.
//!
//! The example tree is embedded at compile time and addressed as
//! `<category>/<name>` through [`find`], or by a unique short name through
//! [`resolve_name`]. [`list`] is the flat catalog `espforge examples` prints;
//! [`materialize`] writes an example as an editable project and never merges
//! into an existing tree.
//!
//! Nothing here prints: presentation lives in the `espforge` binary.

use std::ffi::OsStr;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use include_dir::{Dir, include_dir};

/// The example tree, embedded at compile time from `src/examples`.
pub static EXAMPLES_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/src/examples");

/// Name of the ESPF source file inside each example directory.
const EXAMPLE_FILE: &str = "example.espf.txt";

/// The test harness's cell declaration; never part of a scaffolded project.
const HARNESS_FILE: &str = "example.toml";

/// App logic inside an example, relative to the example directory.
const EXAMPLE_APP: &str = "app/rust/app.rs";

/// Header of the format reference appendix some examples carry at the end.
const FORMAT_REFERENCE: &str = "ESPF FILE FORMAT";

/// A file inside an example, with its path relative to the example directory.
#[derive(Debug)]
pub struct ExampleFile {
    /// Relative path, e.g. `app/rust/app.rs` or `example.espf.txt`.
    pub path: String,
    /// Verbatim embedded contents (examples may contain binaries, e.g. wasm).
    pub contents: &'static [u8],
}

/// A top-level example category, e.g. `01.Basics`.
#[derive(Debug)]
pub struct Category {
    /// Category directory name, e.g. `01.Basics`.
    pub name: String,
    /// Examples directly under this category, sorted by name.
    pub examples: Vec<Example>,
}

/// A single example (a directory containing `example.espf.txt`).
#[derive(Debug)]
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

/// One row of [`list`]: an example with the category it lives in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExampleInfo {
    /// `category/name`, the address [`find`] and `--example` take.
    pub key: String,
    /// Category directory name, e.g. `01.Basics`.
    pub category: String,
    /// Example directory name, e.g. `helloworld`.
    pub name: String,
    /// Human-facing title, when the example has a banner.
    pub title: Option<String>,
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

/// Find an example by `category/name`, e.g. `01.Basics/helloworld`.
pub fn find(example_path: &str) -> Option<&'static Example> {
    let (category_name, example_name) = example_path.split_once('/')?;
    let category = categories().iter().find(|c| c.name == category_name)?;
    category.examples.iter().find(|e| e.name == example_name)
}

/// The whole catalog, category by category and each category by name: the
/// order [`list`] renders and [`resolve_name`] suggests from.
pub fn list() -> Vec<ExampleInfo> {
    categories()
        .iter()
        .flat_map(|category| {
            category.examples.iter().map(move |example| ExampleInfo {
                key: format!("{}/{}", category.name, example.name),
                category: category.name.clone(),
                name: example.name.clone(),
                title: example.title.clone(),
            })
        })
        .collect()
}

/// Resolve user input to the category and example it names.
///
/// A full `category/name` key is taken as written. A bare name is accepted
/// when exactly one example in the catalog has it; no match and several
/// matches are explained, and a typo gets a `did you mean` with the full key.
pub fn resolve_name(input: &str) -> Result<(&'static str, &'static Example), String> {
    resolve_in(categories(), input)
}

fn resolve_in<'a>(catalog: &'a [Category], input: &str) -> Result<(&'a str, &'a Example), String> {
    if let Some((category_name, example_name)) = input.split_once('/') {
        return match catalog.iter().find(|c| c.name == category_name) {
            Some(category) => category
                .examples
                .iter()
                .find(|example| example.name == example_name)
                .map(|example| (category.name.as_str(), example))
                .ok_or_else(|| not_found(catalog, input)),
            None => Err(not_found(catalog, input)),
        };
    }

    let matches: Vec<(&str, &Example)> = catalog
        .iter()
        .flat_map(|category| {
            category
                .examples
                .iter()
                .map(move |example| (category.name.as_str(), example))
        })
        .filter(|(_, example)| example.name == input)
        .collect();

    match matches.len() {
        1 => Ok(matches[0]),
        0 => Err(not_found(catalog, input)),
        _ => {
            let keys: Vec<String> = matches
                .iter()
                .map(|(category, example)| format!("{category}/{}", example.name))
                .collect();
            let list = keys
                .iter()
                .map(|key| format!("  - {key}"))
                .collect::<Vec<_>>()
                .join("\n");
            Err(format!(
                "'{input}' matches more than one example:\n{list}\n   Use the full name, e.g. `--example {}`.",
                keys[0]
            ))
        }
    }
}

fn not_found(catalog: &[Category], input: &str) -> String {
    let hint = suggestion(catalog, input)
        .map(|key| format!(" Did you mean `{key}`?"))
        .unwrap_or_default();
    format!("no example named '{input}'.{hint}\n   Run `espforge examples` to see all examples.")
}

/// The full key of the example whose short name is closest to `input`, when
/// something is close enough to be worth suggesting.
fn suggestion(catalog: &[Category], input: &str) -> Option<String> {
    let limit = 2.max(input.len() / 3);
    catalog
        .iter()
        .flat_map(|category| {
            category
                .examples
                .iter()
                .map(move |example| (category, example))
        })
        .map(|(category, example)| {
            (
                edit_distance(input, &example.name),
                format!("{}/{}", category.name, example.name),
            )
        })
        .filter(|(distance, _)| *distance <= limit)
        .min()
        .map(|(_, key)| key)
}

/// Levenshtein distance, counted in characters.
fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut cur = vec![i];
        for j in 1..=b.len() {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            cur.push((prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost));
        }
        prev = cur;
    }
    prev[b.len()]
}

/// Why [`materialize`] wrote nothing.
#[derive(Debug)]
pub enum MaterializeError {
    /// The destination already holds something; nothing was written.
    Conflict(String),
    /// The OS refused a directory or a write.
    Io(io::Error),
}

impl std::fmt::Display for MaterializeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MaterializeError::Conflict(message) => f.write_str(message),
            MaterializeError::Io(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for MaterializeError {}

impl From<io::Error> for MaterializeError {
    fn from(error: io::Error) -> Self {
        MaterializeError::Io(error)
    }
}

/// What [`materialize`] wrote, relative to the destination.
#[derive(Debug)]
pub struct Materialized {
    /// The project spec, `<name>.espf` (the example's `example.espf.txt`).
    pub spec: PathBuf,
    /// Every file written, in write order.
    pub files: Vec<PathBuf>,
}

/// The spec a scaffolded project starts from: the example's own text with the
/// title banner kept and a trailing `ESPF FILE FORMAT` reference block dropped.
///
/// The appendix documents the format rather than describing the project, so it
/// has no place in a project the user is about to edit.
pub fn project_spec(example: &Example) -> String {
    let lines: Vec<&str> = example.content.lines().collect();
    let cut = lines.iter().enumerate().find_map(|(i, line)| {
        (is_banner(line)
            && lines
                .get(i + 1)
                .is_some_and(|next| next.trim() == FORMAT_REFERENCE))
        .then_some(i)
    });
    let kept = cut.map_or(lines.as_slice(), |i| &lines[..i]);
    format!("{}\n", kept.join("\n").trim_end())
}

/// Write `example` as an editable project under `dest`:
///
/// - `example.espf.txt` becomes `<name>.espf` ([`project_spec`]);
/// - `app/rust/app.rs` becomes `src/app.rs`;
/// - `example.toml` is skipped (it declares test cells, not project files);
/// - every other embedded file is written verbatim at its own path.
///
/// Never merges into an existing tree: `dest` must be absent or empty, and no
/// target file may exist. On a conflict nothing is written.
pub fn materialize(example: &Example, dest: &Path) -> Result<Materialized, MaterializeError> {
    let plan = project_files(example);

    if dest.is_file() {
        return Err(MaterializeError::Conflict(format!(
            "cannot write '{}': not a directory",
            dest.display()
        )));
    }
    if std::fs::read_dir(dest).is_ok_and(|mut entries| entries.next().is_some()) {
        return Err(MaterializeError::Conflict(format!(
            "'{}' already exists and is not empty; pick a new directory name",
            dest.display()
        )));
    }
    for (rel, _) in &plan {
        let target = dest.join(rel);
        if target.exists() {
            return Err(MaterializeError::Conflict(format!(
                "refusing to overwrite '{}' (already exists)",
                target.display()
            )));
        }
    }

    std::fs::create_dir_all(dest)?;
    let mut files = Vec::with_capacity(plan.len());
    for (rel, contents) in &plan {
        let target = dest.join(rel);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&target, contents)?;
        files.push(target);
    }

    Ok(Materialized {
        spec: dest.join(format!("{}.espf", example.name)),
        files,
    })
}

/// The `(relative path, contents)` a scaffold writes, in embedded order.
fn project_files(example: &Example) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    for file in &example.files {
        let rel = match file.path.as_str() {
            HARNESS_FILE => continue,
            EXAMPLE_FILE => format!("{}.espf", example.name),
            EXAMPLE_APP => "src/app.rs".to_string(),
            other => other.to_string(),
        };
        let contents = if file.path == EXAMPLE_FILE {
            project_spec(example).into_bytes()
        } else {
            file.contents.to_vec()
        };
        out.push((rel, contents));
    }
    out
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

    /// Unique scratch directory per test, so tests run in parallel.
    fn scratch(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "espforge-examples-test-{}-{tag}",
            std::process::id()
        ))
    }

    fn fake(name: &str) -> Example {
        Example {
            name: name.into(),
            title: None,
            content: String::new(),
            files: Vec::new(),
        }
    }

    #[test]
    fn examples_embed_and_parse() {
        let categories = categories();
        assert!(!categories.is_empty());
        let basics = &categories[0];
        assert_eq!(basics.name, "01.Basics");
        let helloworld = basics
            .examples
            .iter()
            .find(|e| e.name == "helloworld")
            .expect("01.Basics/helloworld must be embedded");
        assert_eq!(helloworld.title.as_deref(), Some("HELLO WORLD EXAMPLE"));
        assert!(helloworld.content.contains("# espforge"));
    }

    #[test]
    fn find_addresses_examples() {
        let hello = find("01.Basics/helloworld").expect("01.Basics/helloworld must exist");
        assert_eq!(hello.title.as_deref(), Some("HELLO WORLD EXAMPLE"));
        assert!(find("01.Basics/no-such-example").is_none());
        assert!(find("no-such-category/helloworld").is_none());
        assert!(find("no-slash").is_none());
    }

    #[test]
    fn example_records_every_file() {
        let hello = find("01.Basics/helloworld").expect("helloworld must exist");
        let paths: Vec<&str> = hello.files.iter().map(|f| f.path.as_str()).collect();
        for expected in ["example.espf.txt", "app/rust/app.rs", "example.toml"] {
            assert!(paths.contains(&expected), "missing {expected} in {paths:?}");
        }
        let espf = hello
            .files
            .iter()
            .find(|f| f.path == "example.espf.txt")
            .expect("espf must be among the files");
        let text = std::str::from_utf8(espf.contents).expect("espf must be utf8");
        assert_eq!(text, hello.content);
    }

    #[test]
    fn list_is_the_whole_catalog_in_order() {
        let all = list();
        let from_catalog: usize = categories().iter().map(|c| c.examples.len()).sum();
        assert_eq!(all.len(), from_catalog);
        assert!(
            all.windows(2)
                .all(|w| (&w[0].category, &w[0].name) <= (&w[1].category, &w[1].name)),
            "list must keep the catalog's category/name order"
        );
        let hello = all
            .iter()
            .find(|info| info.key == "01.Basics/helloworld")
            .expect("helloworld must be listed");
        assert_eq!(hello.category, "01.Basics");
        assert_eq!(hello.name, "helloworld");
        assert_eq!(hello.title.as_deref(), Some("HELLO WORLD EXAMPLE"));
    }

    #[test]
    fn resolve_name_takes_a_key_or_a_unique_short_name() {
        let (category, example) = resolve_name("01.Basics/helloworld").expect("full key resolves");
        assert_eq!(category, "01.Basics");
        assert_eq!(example.name, "helloworld");

        let (category, example) = resolve_name("blink").expect("short name resolves");
        assert_eq!(format!("{category}/{}", example.name), "01.Basics/blink");
    }

    #[test]
    fn resolve_name_explains_a_typo() {
        let err = resolve_name("blnik").expect_err("a typo must not resolve");
        assert!(err.contains("no example named 'blnik'"), "{err}");
        assert!(err.contains("Did you mean `01.Basics/blink`"), "{err}");
        assert!(err.contains("espforge examples"), "{err}");
    }

    #[test]
    fn resolve_name_refuses_an_ambiguous_short_name() {
        let catalog = vec![
            Category {
                name: "01.Basics".into(),
                examples: vec![fake("blink")],
            },
            Category {
                name: "02.Digital".into(),
                examples: vec![fake("blink")],
            },
        ];
        let err = resolve_in(&catalog, "blink").expect_err("ambiguous must not resolve");
        assert!(err.contains("more than one example"), "{err}");
        assert!(err.contains("  - 01.Basics/blink"), "{err}");
        assert!(err.contains("  - 02.Digital/blink"), "{err}");
        assert!(
            resolve_in(&catalog, "01.Basics/blink").is_ok(),
            "the full key stays unambiguous"
        );
    }

    #[test]
    fn every_short_name_resolves_to_its_own_example() {
        for info in list() {
            let (category, example) =
                resolve_name(&info.name).unwrap_or_else(|e| panic!("{}: {e}", info.key));
            assert_eq!(
                format!("{category}/{}", example.name),
                info.key,
                "short names must be unique across the catalog"
            );
        }
    }

    #[test]
    fn project_spec_keeps_the_banner_and_drops_the_format_reference() {
        let hello = find("01.Basics/helloworld").expect("helloworld must exist");
        let spec = project_spec(hello);
        assert!(spec.starts_with("====================\nHELLO WORLD EXAMPLE"));
        assert!(spec.contains("# espforge"));
        assert!(!spec.contains(FORMAT_REFERENCE), "{spec}");
        assert!(spec.ends_with('\n'));

        // An example without the appendix keeps everything else.
        let ili = find("06.Displays/ili9341").expect("ili9341 must exist");
        let spec = project_spec(ili);
        assert_eq!(spec.trim_end(), ili.content.trim_end());
    }

    #[test]
    fn materialize_writes_a_project_layout() {
        let blink = find("01.Basics/blink").expect("blink must exist");
        let dest = scratch("layout");
        let _ = std::fs::remove_dir_all(&dest);

        let done = materialize(blink, &dest).expect("materialize must succeed");
        assert_eq!(done.spec, dest.join("blink.espf"));
        assert!(dest.join("blink.espf").is_file());
        assert!(dest.join("src").join("app.rs").is_file());
        assert!(dest.join("diagram.json").is_file());
        assert!(!dest.join("example.toml").exists());
        assert!(!dest.join("app").exists());
        for file in &done.files {
            assert!(file.is_file(), "{} missing", file.display());
        }

        let _ = std::fs::remove_dir_all(&dest);
    }

    #[test]
    fn materialize_keeps_binary_example_files() {
        let i2c = find("04.Communication/i2c_example").expect("i2c_example must exist");
        let dest = scratch("binary");
        let _ = std::fs::remove_dir_all(&dest);

        materialize(i2c, &dest).expect("materialize must succeed");
        let wasm = std::fs::read(dest.join("chip.wasm")).expect("chip.wasm written");
        assert!(wasm.starts_with(b"\0asm"), "wasm payload must be verbatim");
        assert!(dest.join("chip").join("chip.zig").is_file());

        let _ = std::fs::remove_dir_all(&dest);
    }

    #[test]
    fn materialize_refuses_to_merge_into_a_non_empty_directory() {
        let blink = find("01.Basics/blink").expect("blink must exist");
        let dest = scratch("conflict");
        let _ = std::fs::remove_dir_all(&dest);
        std::fs::create_dir_all(&dest).unwrap();
        std::fs::write(dest.join("keep.txt"), "mine").unwrap();

        let err = materialize(blink, &dest).expect_err("a non-empty dest must be refused");
        assert!(
            err.to_string().contains("already exists and is not empty"),
            "{err}"
        );
        assert!(!dest.join("blink.espf").exists(), "nothing may be written");
        assert_eq!(
            std::fs::read_to_string(dest.join("keep.txt")).unwrap(),
            "mine"
        );

        let _ = std::fs::remove_dir_all(&dest);
    }

    #[test]
    fn materialize_accepts_an_empty_existing_directory() {
        let blink = find("01.Basics/blink").expect("blink must exist");
        let dest = scratch("empty");
        let _ = std::fs::remove_dir_all(&dest);
        std::fs::create_dir_all(&dest).unwrap();

        materialize(blink, &dest).expect("an empty directory is usable");
        assert!(dest.join("blink.espf").is_file());

        let _ = std::fs::remove_dir_all(&dest);
    }
}
