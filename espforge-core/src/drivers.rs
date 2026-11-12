//! Driver sources: where a project's driver manifests come from, what was
//! read, and the lock that records both.
//!
//! `changes.md` §3 names four tiers — project, an explicit directory, a
//! fetched cache and the bundled schema. This module resolves them in
//! precedence order, and the schema composer refuses a tag declared twice, so a
//! driver is never silently shadowed.
//!
//! It also owns `.espforge/drivers.lock` (version 2): the manifest hash, the
//! package payload hash, and — for a fetched driver — the source, requested
//! revision and resolved commit. `gen` and `build` read manifests and never
//! fetch; only `espforge driver add|update|verify --refresh` writes a source.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use espforge_elements::Board;
use espforge_schema::{Element, Schema};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::fetch::{self, Spec};
use crate::source::Source;

/// Where a manifest came from, in precedence order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    /// `<spec dir>/drivers/<tag>.kdl`.
    Project,
    /// A `--driver-dir` directory.
    Directory,
    /// `$XDG_CACHE_HOME/espforge/drivers/<tag>/<version>/driver.kdl`.
    Cache,
    /// A file the workspace bundles.
    Bundled,
}

impl Tier {
    pub fn as_str(self) -> &'static str {
        match self {
            Tier::Project => "project",
            Tier::Directory => "--driver-dir",
            Tier::Cache => "cache",
            Tier::Bundled => "bundled",
        }
    }
}

/// One manifest file read for this run.
#[derive(Debug, Clone)]
pub struct Manifest {
    pub path: PathBuf,
    pub tier: Tier,
    pub text: String,
}

/// Every manifest a project contributes, in precedence order.
#[derive(Debug, Clone, Default)]
pub struct Manifests {
    pub files: Vec<Manifest>,
    /// `SCHEMA_V1_DEPRECATED` lines, one per v1 file read.
    pub warnings: Vec<String>,
}

impl Manifests {
    /// The `(label, text)` pairs [`espforge_elements::schema::compose`] takes.
    ///
    /// The label is the path, so a duplicate tag can name the two files.
    pub fn sources(&self) -> Vec<(String, String)> {
        self.files
            .iter()
            .map(|file| (file.path.display().to_string(), file.text.clone()))
            .collect()
    }
}

/// The cache root for fetched packages: `$XDG_CACHE_HOME/espforge/drivers`,
/// or `~/.cache/espforge/drivers`.
pub fn cache_root() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache")))?;
    Some(base.join("espforge").join("drivers"))
}

/// Resolve every manifest tier for `source`.
///
/// A missing directory is empty, not an error; an unreadable one is. The cache
/// is read after the explicit directories, so a fetched driver is usable
/// offline once `espforge driver add` has put it there.
pub fn collect(source: &Source, extra_dirs: &[PathBuf]) -> Result<Manifests, String> {
    let mut manifests = Manifests::default();
    if let Some(dir) = &source.dir {
        read_dir(&dir.join("drivers"), Tier::Project, &mut manifests)?;
    }
    for dir in extra_dirs {
        read_dir(dir, Tier::Directory, &mut manifests)?;
    }
    if let Some(cache) = cache_root() {
        read_cache(&cache, &mut manifests)?;
    }
    Ok(manifests)
}

/// Read every `*.kdl` directly inside `dir`, in name order.
fn read_dir(dir: &Path, tier: Tier, manifests: &mut Manifests) -> Result<(), String> {
    if !dir.exists() {
        return Ok(());
    }
    if !dir.is_dir() {
        return Err(format!("'{}' is not a directory", dir.display()));
    }
    let mut entries = std::fs::read_dir(dir)
        .map_err(|e| format!("cannot read '{}': {e}", dir.display()))?
        .collect::<std::io::Result<Vec<_>>>()
        .map_err(|e| e.to_string())?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        if path.is_file() && path.extension().is_some_and(|ext| ext == "kdl") {
            let text = std::fs::read_to_string(&path)
                .map_err(|e| format!("cannot read '{}': {e}", path.display()))?;
            warn_v1(&path, &text, manifests);
            manifests.files.push(Manifest { path, tier, text });
        }
    }
    Ok(())
}

/// Read `<tag>/<version>/driver.kdl` from the cache, in name order.
fn read_cache(cache: &Path, manifests: &mut Manifests) -> Result<(), String> {
    if !cache.is_dir() {
        return Ok(());
    }
    let mut tags = std::fs::read_dir(cache)
        .map_err(|e| format!("cannot read '{}': {e}", cache.display()))?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();
    tags.sort();
    for tag in tags {
        let mut versions = std::fs::read_dir(&tag)
            .map_err(|e| format!("cannot read '{}': {e}", tag.display()))?
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .collect::<Vec<_>>();
        versions.sort();
        for version in versions {
            let path = version.join("driver.kdl");
            if path.is_file() {
                let text = std::fs::read_to_string(&path)
                    .map_err(|e| format!("cannot read '{}': {e}", path.display()))?;
                warn_v1(&path, &text, manifests);
                manifests.files.push(Manifest {
                    path,
                    tier: Tier::Cache,
                    text,
                });
            }
        }
    }
    Ok(())
}

fn warn_v1(path: &Path, text: &str, manifests: &mut Manifests) {
    if text.contains("device_recipe")
        && text
            .lines()
            .any(|line| line.trim_start().starts_with("element "))
    {
        manifests.warnings.push(format!(
            "SCHEMA_V1_DEPRECATED: '{}' uses the v1 `element` format; run `espforge driver migrate` to convert it (v1 support is removed one release after v2 ships)",
            path.display()
        ));
    }
}

// ------------------------------------------------------------------ lockfile

/// The lock format. Version 1 (manifest hash only) is read and upgraded in
/// memory; the next successful `add`/`update` writes version 2.
pub const LOCK_VERSION: u32 = 2;

/// `.espforge/drivers.lock`: the manifests a project was generated from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriversLock {
    pub version: u32,
    pub drivers: BTreeMap<String, LockEntry>,
}

impl DriversLock {
    pub fn new() -> Self {
        Self {
            version: LOCK_VERSION,
            drivers: BTreeMap::new(),
        }
    }
}

impl Default for DriversLock {
    fn default() -> Self {
        Self::new()
    }
}

/// One driver's recorded source, hashes and resolved dependencies.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockEntry {
    /// `local`, `https`, `git`, `registry` or (for a v1 lock) empty.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub kind: String,
    /// The original source (URL or path), or the cache manifest path for a v1
    /// entry upgraded in memory.
    pub source: String,
    /// The requested revision/version, when the source is fetched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requested: Option<String>,
    /// The resolved full Git commit, when the source is a repository.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    /// SHA-256 of the manifest text exactly as it was read.
    pub manifest_sha256: String,
    /// SHA-256 of the fetched payload beside the manifest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload_sha256: Option<String>,
    /// Every dependency the manifest names.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<LockDependency>,
    /// Where the cache entry lives, when it is fetched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_path: Option<PathBuf>,
    /// The v1 `crate` key, kept so an old lock is readable.
    #[serde(rename = "crate", default, skip_serializing_if = "Option::is_none")]
    pub krate: Option<LockCrate>,
    /// The v1 payload hash.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package_sha256: Option<String>,
}

/// One dependency in a lock entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockDependency {
    pub package: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// The crate half of a v1 lock entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockCrate {
    pub package: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// The metadata written beside a cache entry.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct SourceMeta {
    kind: String,
    source: String,
    #[serde(default)]
    requested: Option<String>,
    #[serde(default)]
    commit: Option<String>,
    #[serde(default)]
    payload_sha256: Option<String>,
}

/// A manifest a project actually used, with its current hash.
#[derive(Debug, Clone)]
pub struct UsedDriver {
    pub tag: String,
    pub manifest: PathBuf,
    pub entry: LockEntry,
}

/// The lock path under a project directory.
pub fn lock_path(dir: &Path) -> PathBuf {
    dir.join(".espforge").join("drivers.lock")
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// Read the lock under `dir`; `None` when there is none.
pub fn read_lock(dir: &Path) -> Result<Option<DriversLock>, String> {
    let path = lock_path(dir);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("cannot read '{}': {error}", path.display())),
    };
    let mut lock: DriversLock = serde_json::from_str(&text)
        .map_err(|error| format!("'{}' is not a valid driver lock: {error}", path.display()))?;
    // A v1 entry has no kind; name it for the rest of the run.
    for entry in lock.drivers.values_mut() {
        if entry.kind.is_empty() {
            entry.kind = "local".into();
        }
    }
    Ok(Some(lock))
}

/// Write `lock` under `dir`, creating `.espforge/` when needed.
pub fn write_lock(dir: &Path, lock: &DriversLock) -> Result<(), String> {
    let path = lock_path(dir);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create '{}': {e}", parent.display()))?;
    }
    let mut text = serde_json::to_string_pretty(lock)
        .map_err(|e| format!("cannot encode the driver lock: {e}"))?;
    text.push('\n');
    std::fs::write(&path, text).map_err(|e| format!("cannot write '{}': {e}", path.display()))
}

/// The manifests the composed schema says the project used.
///
/// Bundled elements carry a bundled label that never matches a project file,
/// so only the project's own drivers are recorded.
pub fn used(schema: &Schema, manifests: &Manifests, board: &Board) -> Vec<UsedDriver> {
    let by_label: BTreeMap<String, &Manifest> = manifests
        .files
        .iter()
        .map(|file| (file.path.display().to_string(), file))
        .collect();
    // A cache may hold drivers this project does not use. Record only the
    // tags that actually lowered into the board, so one project's cache cannot
    // leak into another project's lock.
    let tags: std::collections::BTreeSet<&str> = board
        .devices
        .iter()
        .filter_map(espforge_elements::Device::as_bus)
        .map(|device| device.tag.as_str())
        .collect();
    schema
        .elements
        .iter()
        .filter(|element| tags.contains(element.tag.as_str()))
        .filter_map(|element| {
            let label = element.source.as_deref()?;
            let file = by_label.get(label)?;
            Some(UsedDriver {
                tag: element.tag.clone(),
                manifest: file.path.clone(),
                entry: lock_entry(element, file),
            })
        })
        .collect()
}

fn lock_entry(element: &Element, file: &Manifest) -> LockEntry {
    let dependencies = element
        .dependencies
        .iter()
        .map(|dependency| LockDependency {
            package: dependency.package.clone(),
            path: dependency.path.clone(),
        })
        .collect();
    if file.tier == Tier::Cache {
        if let Ok(meta) = read_source_meta(&file.path) {
            return LockEntry {
                kind: meta.kind,
                source: meta.source,
                requested: meta.requested,
                commit: meta.commit,
                manifest_sha256: sha256_hex(file.text.as_bytes()),
                payload_sha256: meta.payload_sha256,
                dependencies,
                cache_path: Some(file.path.clone()),
                krate: element.package.as_ref().map(|package| LockCrate {
                    package: package.package.clone(),
                    version: package.version.clone(),
                    path: package.path.clone(),
                }),
                package_sha256: None,
            };
        }
        return LockEntry {
            kind: "cache".into(),
            source: file.path.display().to_string(),
            manifest_sha256: sha256_hex(file.text.as_bytes()),
            dependencies,
            cache_path: Some(file.path.clone()),
            ..LockEntry::default()
        };
    }
    LockEntry {
        kind: "local".into(),
        source: file.path.display().to_string(),
        manifest_sha256: sha256_hex(file.text.as_bytes()),
        dependencies,
        krate: element.package.as_ref().map(|package| LockCrate {
            package: package.package.clone(),
            version: package.version.clone(),
            path: package.path.clone(),
        }),
        ..LockEntry::default()
    }
}

/// The lock a successful generation writes: exactly what this run used.
pub fn record(used: &[UsedDriver]) -> DriversLock {
    let mut lock = DriversLock::new();
    for driver in used {
        lock.drivers
            .insert(driver.tag.clone(), driver.entry.clone());
    }
    lock
}

/// Why a lock does not match the files on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LockProblem {
    /// The lock has no entry for a manifest the project uses.
    MissingEntry { tag: String, path: PathBuf },
    /// The manifest changed since the entry was recorded.
    Stale {
        tag: String,
        path: PathBuf,
        expected: String,
        found: String,
    },
}

impl std::fmt::Display for LockProblem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LockProblem::MissingEntry { tag, path } => write!(
                f,
                "DRIVER_LOCK_MISSING: `<{tag}>` in '{}' has no `.espforge/drivers.lock` entry\n   \
                 fix: espforge driver add --local '{}'",
                path.display(),
                path.display()
            ),
            LockProblem::Stale {
                tag,
                path,
                expected,
                found,
            } => write!(
                f,
                "DRIVER_LOCK_STALE: `<{tag}>` in '{}' changed since it was recorded\n   \
                 expected sha256 {expected}\n   found    sha256 {found}\n   \
                 fix: espforge driver update {tag}",
                path.display()
            ),
        }
    }
}

/// Compare the recorded hashes with the files this run read.
///
/// `None` means the project has no lock yet: the first generation records one,
/// and later runs verify it. A lock that exists but lacks an entry is refused.
pub fn verify(lock: Option<&DriversLock>, used: &[UsedDriver]) -> Result<(), Vec<LockProblem>> {
    let Some(lock) = lock else {
        return Ok(());
    };
    let mut problems = Vec::new();
    for driver in used {
        match lock.drivers.get(&driver.tag) {
            None => problems.push(LockProblem::MissingEntry {
                tag: driver.tag.clone(),
                path: driver.manifest.clone(),
            }),
            Some(entry) => {
                if entry.manifest_sha256 != driver.entry.manifest_sha256 {
                    problems.push(LockProblem::Stale {
                        tag: driver.tag.clone(),
                        path: driver.manifest.clone(),
                        expected: driver.entry.manifest_sha256.clone(),
                        found: entry.manifest_sha256.clone(),
                    });
                }
            }
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// Resolve every driver dependency's `path` to an absolute path.
///
/// The path is relative to the manifest that declared it. It must stay inside
/// the project or inside that manifest's directory, so a fetched manifest
/// cannot name a crate anywhere on the machine.
pub fn resolve_package_paths(board: &mut Board, project_dir: &Path) -> Result<(), String> {
    let root = project_dir
        .canonicalize()
        .unwrap_or_else(|_| project_dir.to_path_buf());
    for device in board.devices.iter_mut().filter_map(|device| match device {
        espforge_elements::Device::Bus(device) => Some(device),
        espforge_elements::Device::Pin(_) => None,
    }) {
        let base = device
            .driver
            .manifest
            .as_deref()
            .and_then(|manifest| Path::new(manifest).parent())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| root.clone());
        let base = base.canonicalize().unwrap_or(base);
        let allowed = if base.starts_with(&root) {
            root.clone()
        } else {
            base.parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| base.clone())
        };
        for dependency in &mut device.driver.dependencies {
            let Some(path) = dependency.path.clone() else {
                continue;
            };
            let resolved = resolve_one(&base, &allowed, &device.tag, &dependency.package, &path)?;
            dependency.path = Some(resolved);
        }
        if let Some(package) = &mut device.driver.package
            && let Some(path) = package.path.clone()
        {
            let resolved = resolve_one(&base, &allowed, &device.tag, &package.package, &path)?;
            package.path = Some(resolved);
        }
    }
    Ok(())
}

fn resolve_one(
    base: &Path,
    allowed: &Path,
    tag: &str,
    package: &str,
    path: &str,
) -> Result<String, String> {
    let joined = base.join(path);
    let canonical = joined.canonicalize().map_err(|error| {
        format!(
            "driver `<{tag}>`: cannot read crate path '{package}' at '{}': {error}",
            joined.display()
        )
    })?;
    if !canonical.starts_with(allowed) && !canonical.starts_with(base) {
        return Err(format!(
            "driver `<{tag}>`: crate path '{path}' escapes '{}'",
            allowed.display()
        ));
    }
    Ok(canonical.to_string_lossy().replace('\\', "/"))
}

// ------------------------------------------------------- add/fetch/verify

/// Read a manifest and return the one element it declares.
///
/// Composed against the core vocabulary only: a project manifest may not
/// redeclare a bundled driver tag, and `add` has to say that before writing
/// anything.
fn manifest_element(path: &Path) -> Result<(String, Element), String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("cannot read '{}': {error}", path.display()))?;
    manifest_element_text(path, &text)
}

fn manifest_element_text(path: &Path, text: &str) -> Result<(String, Element), String> {
    let schema = espforge_schema::load_all(&[
        espforge_schema::SCHEMA_KDL,
        espforge_schema::SERVICES_SCHEMA_KDL,
        text,
    ])
    .map_err(|errors| {
        let mut message = format!("'{}' is not a driver manifest:", path.display());
        for error in errors {
            message.push_str(&format!("\n  {}: {}", error.span.start, error.message));
        }
        message
    })?;
    let core = espforge_schema::schema();
    let element = schema
        .elements
        .iter()
        .find(|element| core.element(&element.tag).is_none())
        .ok_or_else(|| {
            format!(
                "'{}' declares no `driver`/`element` beyond the core schema",
                path.display()
            )
        })?;
    Ok((element.tag.clone(), element.clone()))
}

/// Copy `manifest` into `<project>/drivers/` (unless `local`) and record it.
pub fn add(project_dir: &Path, manifest: &Path, local: bool) -> Result<String, String> {
    let (tag, element) = manifest_element(manifest)?;
    let text = std::fs::read(manifest)
        .map_err(|error| format!("cannot read '{}': {error}", manifest.display()))?;

    let stored = if local {
        manifest.to_path_buf()
    } else {
        let dir = project_dir.join("drivers");
        std::fs::create_dir_all(&dir)
            .map_err(|error| format!("cannot create '{}': {error}", dir.display()))?;
        let target = dir.join(format!("{tag}.kdl"));
        std::fs::write(&target, &text)
            .map_err(|error| format!("cannot write '{}': {error}", target.display()))?;
        target
    };

    let mut lock = read_lock(project_dir)?.unwrap_or_default();
    lock.version = LOCK_VERSION;
    lock.drivers.insert(
        tag.clone(),
        LockEntry {
            kind: "local".into(),
            source: stored.display().to_string(),
            manifest_sha256: sha256_hex(&text),
            dependencies: element
                .dependencies
                .iter()
                .map(|dependency| LockDependency {
                    package: dependency.package.clone(),
                    path: dependency.path.clone(),
                })
                .collect(),
            krate: element.package.as_ref().map(|package| LockCrate {
                package: package.package.clone(),
                version: package.version.clone(),
                path: package.path.clone(),
            }),
            ..LockEntry::default()
        },
    );
    write_lock(project_dir, &lock)?;
    let crate_name = element
        .package
        .as_ref()
        .map(|package| package.package.as_str())
        .unwrap_or("espforge-drivers");
    Ok(format!(
        "added `<{tag}>` from '{}' -> {} (sha256 {})",
        manifest.display(),
        crate_name,
        sha256_hex(&text)
    ))
}

/// `driver add` for every source kind.
#[allow(clippy::too_many_arguments)]
pub fn add_spec(
    project_dir: &Path,
    source: &str,
    rev: Option<&str>,
    registry: Option<&str>,
    allow_local: bool,
    local: bool,
    sha256: Option<&str>,
    version: Option<&str>,
    dry_run: bool,
) -> Result<String, String> {
    let spec = fetch::parse_source(source, rev, registry, allow_local)?;
    match spec {
        Spec::Path(path) => {
            if dry_run {
                let (tag, element) = manifest_element(&path)?;
                return Ok(format!(
                    "validated `<{tag}>` from '{}' ({} dependencies); nothing written",
                    path.display(),
                    element.dependencies.len()
                ));
            }
            add(project_dir, &path, local)
        }
        Spec::Archive {
            url,
            sha256: pinned,
        } => fetch_archive(
            project_dir,
            &url,
            sha256.or(pinned.as_deref()),
            version,
            "https",
            &url,
            Some(&url),
            dry_run,
        ),
        Spec::Git { url, rev } => fetch_git(project_dir, &url, &rev, version, dry_run),
        Spec::Registry {
            index,
            tag,
            version,
        } => {
            let (url, sha) = fetch::registry_entry(&index, &tag, &version)?;
            fetch_archive(
                project_dir,
                &url,
                sha.as_deref(),
                Some(&version),
                "registry",
                &index,
                Some(&format!("{tag}@{version}")),
                dry_run,
            )
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn fetch_archive(
    project_dir: &Path,
    url: &str,
    sha256: Option<&str>,
    version: Option<&str>,
    kind: &str,
    source: &str,
    requested: Option<&str>,
    dry_run: bool,
) -> Result<String, String> {
    let bytes = fetch::download(url)?;
    if let Some(expected) = sha256 {
        let found = sha256_hex(&bytes);
        if !expected.eq_ignore_ascii_case(&found) {
            return Err(format!(
                "DRIVER_CHECKSUM_MISMATCH: '{url}'\n   expected sha256 {expected}\n   found    sha256 {found}"
            ));
        }
    }
    let temp = temp_dir("archive")?;
    let result = (|| {
        fetch::extract_archive(&bytes, &temp)?;
        let manifest = fetch::find_manifest(&temp)?;
        let text = std::fs::read_to_string(&manifest)
            .map_err(|error| format!("cannot read '{}': {error}", manifest.display()))?;
        let (tag, element) = manifest_element_text(&manifest, &text)?;
        let payload_root = manifest.parent().unwrap_or(&temp).to_path_buf();
        if dry_run {
            return Ok(format!(
                "validated `<{tag}>` from '{url}' ({} dependencies); nothing written",
                element.dependencies.len()
            ));
        }
        let version = version
            .map(str::to_string)
            .or_else(|| {
                element
                    .dependencies
                    .iter()
                    .find_map(|dependency| dependency.version.clone())
            })
            .unwrap_or_else(|| "0.0.0".to_string());
        install_cache(
            project_dir,
            &tag,
            &version,
            &text,
            &payload_root,
            kind,
            source,
            requested,
            None,
        )
    })();
    let _ = std::fs::remove_dir_all(&temp);
    result
}

fn fetch_git(
    project_dir: &Path,
    url: &str,
    rev: &str,
    version: Option<&str>,
    dry_run: bool,
) -> Result<String, String> {
    let temp = temp_dir("git")?;
    let result = (|| {
        let commit = fetch::git_checkout(url, rev, &temp)?;
        let manifest = fetch::find_manifest(&temp)?;
        let text = std::fs::read_to_string(&manifest)
            .map_err(|error| format!("cannot read '{}': {error}", manifest.display()))?;
        let (tag, element) = manifest_element_text(&manifest, &text)?;
        let payload_root = manifest.parent().unwrap_or(&temp).to_path_buf();
        if dry_run {
            return Ok(format!(
                "validated `<{tag}>` at {commit} ({} dependencies); nothing written",
                element.dependencies.len()
            ));
        }
        let version = version
            .unwrap_or(&commit[..commit.len().min(12)])
            .to_string();
        install_cache(
            project_dir,
            &tag,
            &version,
            &text,
            &payload_root,
            "git",
            url,
            Some(rev),
            Some(&commit),
        )
    })();
    let _ = std::fs::remove_dir_all(&temp);
    result
}

fn temp_dir(label: &str) -> Result<PathBuf, String> {
    let root = cache_root().ok_or_else(|| "cannot locate the cache directory".to_string())?;
    std::fs::create_dir_all(&root)
        .map_err(|error| format!("cannot create '{}': {error}", root.display()))?;
    let path = root.join(format!(
        ".tmp-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default()
    ));
    std::fs::create_dir_all(&path)
        .map_err(|error| format!("cannot create '{}': {error}", path.display()))?;
    Ok(path)
}

#[allow(clippy::too_many_arguments)]
fn install_cache(
    project_dir: &Path,
    tag: &str,
    version: &str,
    manifest_text: &str,
    payload_root: &Path,
    kind: &str,
    source: &str,
    requested: Option<&str>,
    commit: Option<&str>,
) -> Result<String, String> {
    let root = cache_root().ok_or_else(|| "cannot locate the cache directory".to_string())?;
    let dir = root.join(tag).join(version);
    if dir.exists() {
        std::fs::remove_dir_all(&dir)
            .map_err(|error| format!("cannot replace '{}': {error}", dir.display()))?;
    }
    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("cannot create '{}': {error}", dir.display()))?;
    fetch::copy_tree(payload_root, &dir)?;
    let manifest_path = dir.join("driver.kdl");
    std::fs::write(&manifest_path, manifest_text)
        .map_err(|error| format!("cannot write '{}': {error}", manifest_path.display()))?;
    let payload_sha256 = fetch::payload_hash(&dir)?;

    let meta = SourceMeta {
        kind: kind.into(),
        source: source.into(),
        requested: requested.map(str::to_string),
        commit: commit.map(str::to_string),
        payload_sha256: Some(payload_sha256.clone()),
    };
    let meta_text = serde_json::to_string_pretty(&meta)
        .map_err(|error| format!("cannot encode cache metadata: {error}"))?;
    std::fs::write(dir.join("source.json"), format!("{meta_text}\n"))
        .map_err(|error| format!("cannot write cache metadata: {error}"))?;

    let (_, element) = manifest_element_text(&manifest_path, manifest_text)?;
    let manifest_sha256 = sha256_hex(manifest_text.as_bytes());
    let mut lock = read_lock(project_dir)?.unwrap_or_default();
    lock.version = LOCK_VERSION;
    lock.drivers.insert(
        tag.to_string(),
        LockEntry {
            kind: kind.into(),
            source: source.into(),
            requested: requested.map(str::to_string),
            commit: commit.map(str::to_string),
            manifest_sha256: manifest_sha256.clone(),
            payload_sha256: Some(payload_sha256.clone()),
            dependencies: element
                .dependencies
                .iter()
                .map(|dependency| LockDependency {
                    package: dependency.package.clone(),
                    path: dependency.path.clone(),
                })
                .collect(),
            cache_path: Some(manifest_path.clone()),
            krate: element.package.as_ref().map(|package| LockCrate {
                package: package.package.clone(),
                version: package.version.clone(),
                path: package.path.clone(),
            }),
            package_sha256: None,
        },
    );
    write_lock(project_dir, &lock)?;
    Ok(format!(
        "added `<{tag}>` {version} from {source} -> '{}' (manifest {manifest_sha256}, payload {payload_sha256})",
        dir.display()
    ))
}

fn read_source_meta(manifest: &Path) -> Result<SourceMeta, String> {
    let path = manifest
        .parent()
        .map(|parent| parent.join("source.json"))
        .ok_or_else(|| format!("'{}' has no cache directory", manifest.display()))?;
    let text = std::fs::read_to_string(&path)
        .map_err(|error| format!("cannot read '{}': {error}", path.display()))?;
    serde_json::from_str(&text)
        .map_err(|error| format!("'{}' is not valid source metadata: {error}", path.display()))
}

/// One `tag -> kind, crate, source, short hash` line per recorded driver.
pub fn list(project_dir: &Path) -> Result<Vec<String>, String> {
    let lock = read_lock(project_dir)?.unwrap_or_default();
    Ok(lock
        .drivers
        .iter()
        .map(|(tag, entry)| {
            let crate_name = entry
                .krate
                .as_ref()
                .map(|krate| krate.package.as_str())
                .unwrap_or("espforge-drivers");
            let short = &entry.manifest_sha256[..entry.manifest_sha256.len().min(12)];
            format!(
                "{tag}\t{}\t{crate_name}\t{}\t{short}",
                if entry.kind.is_empty() {
                    "local"
                } else {
                    &entry.kind
                },
                entry.source
            )
        })
        .collect())
}

/// Re-read a local source, or re-fetch a recorded remote one.
pub fn update(
    project_dir: &Path,
    tag: Option<&str>,
    allow_local: bool,
) -> Result<Vec<String>, String> {
    let lock = read_lock(project_dir)?
        .ok_or_else(|| format!("'{}' has no driver lock", project_dir.display()))?;
    let tags: Vec<String> = match tag {
        Some(tag) => {
            if !lock.drivers.contains_key(tag) {
                return Err(format!("no driver `<{tag}>` in the lock"));
            }
            vec![tag.to_string()]
        }
        None => lock.drivers.keys().cloned().collect(),
    };
    let mut updated = Vec::new();
    for tag in tags {
        let entry = lock
            .drivers
            .get(&tag)
            .cloned()
            .ok_or_else(|| format!("no driver `<{tag}>` in the lock"))?;
        match entry.kind.as_str() {
            "git" => {
                let requested = entry
                    .requested
                    .clone()
                    .ok_or_else(|| format!("`<{tag}>` records no git revision"))?;
                let line = add_spec(
                    project_dir,
                    &format!("git+{}#{requested}", entry.source),
                    None,
                    None,
                    allow_local,
                    false,
                    None,
                    None,
                    false,
                )?;
                updated.push(line);
            }
            "https" | "registry" => {
                let line = if entry.kind == "registry" {
                    let requested = entry
                        .requested
                        .clone()
                        .ok_or_else(|| format!("`<{tag}>` records no registry version"))?;
                    add_spec(
                        project_dir,
                        &requested,
                        None,
                        Some(&entry.source),
                        allow_local,
                        false,
                        None,
                        None,
                        false,
                    )?
                } else {
                    add_spec(
                        project_dir,
                        &entry.source,
                        None,
                        None,
                        allow_local,
                        false,
                        None,
                        None,
                        false,
                    )?
                };
                updated.push(line);
            }
            _ => {
                let path = PathBuf::from(&entry.source);
                let text = std::fs::read(&path).map_err(|error| {
                    format!(
                        "cannot re-read `<{tag}>` from '{}': {error}\n   fix: espforge driver add '{}'",
                        path.display(),
                        path.display()
                    )
                })?;
                let (read_tag, element) = manifest_element(&path)?;
                if read_tag != tag {
                    return Err(format!(
                        "'{}' now declares `<{read_tag}>`, but the lock records `<{tag}>`",
                        path.display()
                    ));
                }
                let mut lock = read_lock(project_dir)?.unwrap_or_default();
                let hash = {
                    let entry = lock
                        .drivers
                        .get_mut(&tag)
                        .ok_or_else(|| format!("no driver `<{tag}>` in the lock"))?;
                    entry.manifest_sha256 = sha256_hex(&text);
                    entry.dependencies = element
                        .dependencies
                        .iter()
                        .map(|dependency| LockDependency {
                            package: dependency.package.clone(),
                            path: dependency.path.clone(),
                        })
                        .collect();
                    entry.krate = element.package.as_ref().map(|package| LockCrate {
                        package: package.package.clone(),
                        version: package.version.clone(),
                        path: package.path.clone(),
                    });
                    entry.manifest_sha256.clone()
                };
                write_lock(project_dir, &lock)?;
                updated.push(format!("{tag}\t{hash}"));
            }
        }
    }
    Ok(updated)
}

/// Re-hash cache contents and compare them with the lock.
pub fn verify_cache(
    project_dir: &Path,
    refresh: bool,
    allow_local: bool,
) -> Result<Vec<String>, String> {
    if refresh {
        return update(project_dir, None, allow_local);
    }
    let lock = read_lock(project_dir)?
        .ok_or_else(|| format!("'{}' has no driver lock", project_dir.display()))?;
    let mut lines = Vec::new();
    for (tag, entry) in &lock.drivers {
        if let Some(cache_path) = &entry.cache_path {
            let text = std::fs::read(cache_path).map_err(|error| {
                format!(
                    "DRIVER_LOCK_STALE: cannot read cached `<{tag}>` at '{}': {error}",
                    cache_path.display()
                )
            })?;
            let found = sha256_hex(&text);
            if found != entry.manifest_sha256 {
                return Err(format!(
                    "DRIVER_LOCK_STALE: cached `<{tag}>` manifest changed\n   expected sha256 {}\n   found    sha256 {found}",
                    entry.manifest_sha256
                ));
            }
            if let Some(expected) = &entry.payload_sha256 {
                let root = cache_path.parent().unwrap_or(Path::new("."));
                let found = fetch::payload_hash(root)?;
                if &found != expected {
                    return Err(format!(
                        "DRIVER_LOCK_STALE: cached `<{tag}>` payload changed\n   expected sha256 {expected}\n   found    sha256 {found}"
                    ));
                }
            }
            lines.push(format!("{tag}\tok (manifest {found})"));
        } else {
            let path = PathBuf::from(&entry.source);
            let text = std::fs::read(&path).map_err(|error| {
                format!(
                    "DRIVER_LOCK_STALE: cannot read `<{tag}>` at '{}': {error}",
                    path.display()
                )
            })?;
            let found = sha256_hex(&text);
            if found != entry.manifest_sha256 {
                return Err(format!(
                    "DRIVER_LOCK_STALE: `<{tag}>` changed since it was recorded\n   expected sha256 {}\n   found    sha256 {found}",
                    entry.manifest_sha256
                ));
            }
            lines.push(format!("{tag}\tok (manifest {found})"));
        }
    }
    Ok(lines)
}

/// Forget `tag`, deleting its copy in `drivers/` and optionally its cache.
pub fn remove(project_dir: &Path, tag: &str, prune_cache: bool) -> Result<(), String> {
    let mut lock = read_lock(project_dir)?
        .ok_or_else(|| format!("'{}' has no driver lock", project_dir.display()))?;
    let entry = lock
        .drivers
        .remove(tag)
        .ok_or_else(|| format!("no driver `<{tag}>` in the lock"))?;
    let copy = project_dir.join("drivers").join(format!("{tag}.kdl"));
    if copy.is_file() {
        std::fs::remove_file(&copy)
            .map_err(|error| format!("cannot delete '{}': {error}", copy.display()))?;
    }
    if prune_cache
        && let Some(cache_path) = entry.cache_path
        && let Some(version) = cache_path.parent()
    {
        let used = lock
            .drivers
            .values()
            .filter_map(|entry| entry.cache_path.as_ref())
            .any(|path| path.starts_with(version));
        if !used {
            let _ = std::fs::remove_dir_all(version);
        }
    }
    write_lock(project_dir, &lock)
}

// -------------------------------------------------------------- migration

/// Rewrite a v1 `element` manifest as a v2 `driver` file.
///
/// Returns the new text; the caller prints it or writes it with `--write`.
/// Recipes with a pin template v2 cannot express (input pins, arbitrary Rust)
/// are refused with a named reason.
pub fn migrate(path: &Path) -> Result<String, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("cannot read '{}': {error}", path.display()))?;
    let schema = espforge_schema::load_all(&[
        espforge_schema::SCHEMA_KDL,
        espforge_schema::SERVICES_SCHEMA_KDL,
        &text,
    ])
    .map_err(|errors| {
        let mut message = format!("'{}' is not a v1 driver manifest:", path.display());
        for error in errors {
            message.push_str(&format!("\n  {}: {}", error.span.start, error.message));
        }
        message
    })?;
    let core = espforge_schema::schema();
    let mut out = String::new();
    for element in &schema.elements {
        if core.element(&element.tag).is_some() {
            continue;
        }
        let migrated = migrate_element(element)?;
        out.push_str(&migrated);
        out.push('\n');
    }
    if out.trim().is_empty() {
        return Err(format!(
            "'{}' declares no driver beyond the core schema",
            path.display()
        ));
    }
    Ok(out)
}

fn migrate_element(element: &Element) -> Result<String, String> {
    let recipe = element
        .device_recipe
        .as_ref()
        .ok_or_else(|| format!("`<{}>` is not a device recipe", element.tag))?;
    let mut out = String::new();
    out.push_str(&format!("driver \"{}\" {{\n", element.tag));
    out.push_str("    format_version 2\n");
    out.push_str(&format!("    summary \"{}\"\n", escape(&element.summary)));
    if let Some(description) = &element.description {
        out.push_str(&format!("    description \"{}\"\n", escape(description)));
    }
    out.push_str(&format!("    category \"{}\"\n", escape(&recipe.kind)));

    if let Some(package) = &element.package {
        out.push_str(&format!("    dependency \"{}\" {{\n", package.package));
        if let Some(version) = &package.version {
            out.push_str(&format!("        version \"{}\"\n", escape(version)));
        }
        if let Some(path) = &package.path {
            out.push_str(&format!("        path \"{}\"\n", escape(path)));
        }
        if let Some(git) = &package.git {
            out.push_str(&format!("        git \"{}\"\n", escape(git)));
        }
        if let Some(rev) = &package.rev {
            out.push_str(&format!("        rev \"{}\"\n", escape(rev)));
        }
        if !package.features.is_empty() {
            out.push_str("        features ");
            for feature in &package.features {
                out.push_str(&format!("\"{}\" ", escape(feature)));
            }
            out.push('\n');
        }
        out.push_str("    }\n");
    }
    if element.package.is_none() {
        for dependency in &element.dependencies {
            if dependency.package == "espforge-drivers" {
                continue;
            }
            out.push_str(&format!("    dependency \"{}\" {{\n", dependency.package));
            match (&dependency.version, &dependency.path, &dependency.git) {
                (Some(version), _, _) => {
                    out.push_str(&format!("        version \"{}\"\n", escape(version)))
                }
                (_, Some(path), _) => out.push_str(&format!("        path \"{}\"\n", escape(path))),
                (_, _, Some(git)) => out.push_str(&format!("        git \"{}\"\n", escape(git))),
                _ => {
                    out.push_str("        version \"0.8.1\"\n");
                }
            }
            if let Some(rev) = &dependency.rev {
                out.push_str(&format!("        rev \"{}\"\n", escape(rev)));
            }
            out.push_str("    }\n");
        }
    } else {
        for dependency in &element.dependencies {
            if Some(&dependency.package) == element.package.as_ref().map(|p| &p.package) {
                continue;
            }
            out.push_str(&format!("    dependency \"{}\" {{\n", dependency.package));
            if let Some(version) = &dependency.version {
                out.push_str(&format!("        version \"{}\"\n", escape(version)));
            } else {
                out.push_str("        version \"0.8.1\"\n");
            }
            out.push_str("    }\n");
        }
    }

    match recipe.bus.as_str() {
        "i2c" => {
            let mut inner = String::new();
            match recipe.address {
                espforge_schema::AddressNeed::Fixed(address) => {
                    inner.push_str(&format!("        address fixed=\"0x{address:02X}\"\n"));
                }
                espforge_schema::AddressNeed::Any => {
                    inner.push_str("        address");
                    if let Some(default) = recipe.address_default {
                        inner.push_str(&format!(" default=\"0x{default:02X}\""));
                    }
                    if let Some((low, high)) = recipe.address_allowed {
                        inner.push_str(&format!(" allowed=\"0x{low:02X}..=0x{high:02X}\""));
                    }
                    inner.push('\n');
                }
                espforge_schema::AddressNeed::None => {}
            }
            if inner.is_empty() {
                out.push_str("    bus \"i2c\"\n");
            } else {
                out.push_str("    bus \"i2c\" {\n");
                out.push_str(&inner);
                out.push_str("    }\n");
            }
        }
        "spi" => {
            out.push_str("    bus \"spi\" {\n");
            if recipe.spi_config {
                let mode = element
                    .attrs
                    .iter()
                    .find(|attr| attr.name == "mode")
                    .and_then(|attr| attr.default.clone())
                    .unwrap_or_else(|| "0".into());
                let frequency = element
                    .attrs
                    .iter()
                    .find(|attr| attr.name == "frequency")
                    .and_then(|attr| attr.default.clone())
                    .unwrap_or_else(|| "40000kHz".into());
                out.push_str(&format!("        mode {mode}\n"));
                out.push_str(&format!("        frequency \"{}\"\n", escape(&frequency)));
            }
            if recipe.cs_polarity == espforge_schema::CsPolarity::High {
                out.push_str("        cs_polarity \"active-high\"\n");
            }
            out.push_str("    }\n");
        }
        "uart" => out.push_str("    bus \"uart\"\n"),
        other => {
            return Err(format!(
                "`<{}>` binds to unknown bus `{other}`",
                element.tag
            ));
        }
    }

    let mut pin_names = Vec::new();
    for arg in recipe.args.iter().filter(|arg| arg.pin.is_some()) {
        let template = arg.pin.as_deref().unwrap();
        if !template.contains("esp_hal::gpio::Output::new") {
            return Err(format!(
                "`<{}>` pin `{}` is not an output pin v2 can express\n   fix: keep it as a v1 manifest or migrate it by hand",
                element.tag, arg.name
            ));
        }
        let level = if template.contains("Level::High") {
            "high"
        } else {
            "low"
        };
        let optional = element
            .attrs
            .iter()
            .find(|attr| attr.name == arg.name)
            .is_some_and(|attr| !attr.required);
        pin_names.push(arg.name.clone());
        out.push_str(&format!(
            "    pin \"{}\" direction=\"output\" default_level=\"{level}\"{}\n",
            arg.name,
            if optional { " optional=#true" } else { "" }
        ));
    }

    out.push_str("    codegen {\n");
    if !recipe.feature.is_empty() {
        out.push_str(&format!(
            "        feature \"{}\"\n",
            escape(&recipe.feature)
        ));
    }
    if recipe.runtimes.blocking {
        out.push_str("        runtime \"blocking\" {\n");
        out.push_str(&format!("            type \"{}\"\n", escape(&recipe.ty)));
        out.push_str(&format!(
            "            constructor \"{}\"\n",
            escape(&constructor(recipe, &recipe.new))
        ));
        if let Some(endpoint) = &recipe.endpoint {
            out.push_str(&format!("            endpoint \"{}\"\n", escape(endpoint)));
        }
        out.push_str("        }\n");
    }
    if recipe.runtimes.asynchronous {
        out.push_str("        runtime \"async\" {\n");
        if let Some(feature) = &recipe.async_mode.feature {
            out.push_str(&format!("            feature \"{}\"\n", escape(feature)));
        }
        out.push_str(&format!(
            "            type \"{}\"\n",
            escape(recipe.async_mode.ty.as_deref().unwrap_or(&recipe.ty))
        ));
        out.push_str(&format!(
            "            constructor \"{}\"\n",
            escape(&constructor(
                recipe,
                recipe.async_mode.new.as_deref().unwrap_or(&recipe.new)
            ))
        ));
        if let Some(endpoint) = &recipe.async_mode.endpoint {
            out.push_str(&format!("            endpoint \"{}\"\n", escape(endpoint)));
        }
        out.push_str("        }\n");
    }
    for arg in recipe.args.iter().filter(|arg| arg.map.is_some()) {
        if pin_names.contains(&arg.name) {
            continue;
        }
        out.push_str(&format!("        map \"{}\" {{\n", arg.name));
        let is_type = recipe.ty.contains(&format!("{{{}}}", arg.name))
            || recipe
                .async_mode
                .ty
                .as_deref()
                .is_some_and(|ty| ty.contains(&format!("{{{}}}", arg.name)));
        for (spelling, value) in arg.map.as_ref().unwrap() {
            out.push_str(&format!(
                "            \"{}\" {{ {} \"{}\" }}\n",
                escape(spelling),
                if is_type { "type" } else { "expr" },
                escape(value)
            ));
        }
        out.push_str("        }\n");
    }
    out.push_str("    }\n");

    for attr in &element.attrs {
        if matches!(
            attr.name.as_str(),
            "id" | "bus" | "address" | "cs" | "mode" | "frequency"
        ) || pin_names.contains(&attr.name)
        {
            continue;
        }
        let kind = match &attr.kind {
            espforge_schema::AttrKind::Ident => "ident".to_string(),
            espforge_schema::AttrKind::Str => "str".to_string(),
            espforge_schema::AttrKind::Pin => "pin".to_string(),
            espforge_schema::AttrKind::Freq => "freq".to_string(),
            espforge_schema::AttrKind::Hex => "hex".to_string(),
            espforge_schema::AttrKind::U32 => "u32".to_string(),
            espforge_schema::AttrKind::Bool => "bool".to_string(),
            espforge_schema::AttrKind::Choice(_) => "choice".to_string(),
            espforge_schema::AttrKind::Ref { .. } => "ref".to_string(),
            espforge_schema::AttrKind::PinOrRef => "pin_or_ref".to_string(),
            espforge_schema::AttrKind::List(_) => "list".to_string(),
        };
        out.push_str(&format!(
            "    attr \"{}\" kind=\"{kind}\"{}{}",
            attr.name,
            if let Some(default) = &attr.default {
                format!(" default=\"{}\"", escape(default))
            } else {
                String::new()
            },
            if attr.required { " required=#true" } else { "" }
        ));
        let mut inner = String::new();
        if let espforge_schema::AttrKind::Choice(choices) = &attr.kind {
            inner.push_str("        choices ");
            for choice in choices {
                inner.push_str(&format!("\"{}\" ", escape(choice)));
            }
            inner.push('\n');
        }
        if let Some(help) = &attr.help {
            inner.push_str(&format!("        help \"{}\"\n", escape(help)));
        }
        if inner.is_empty() {
            out.push('\n');
        } else {
            out.push_str(" {\n");
            out.push_str(&inner);
            out.push_str("    }\n");
        }
    }

    for example in &element.examples {
        out.push_str(&format!(
            "    example \"{}\" \"{}\"\n",
            escape(&example.title),
            escape(&example.fragment)
        ));
    }
    for target in &element.see_also {
        out.push_str(&format!("    see_also \"{}\"\n", escape(target)));
    }
    out.push_str("}\n");
    Ok(out)
}

fn constructor(recipe: &espforge_schema::DeviceRecipe, new: &str) -> String {
    let mut args = vec!["{bus_device}".to_string()];
    if matches!(
        recipe.address,
        espforge_schema::AddressNeed::Any | espforge_schema::AddressNeed::Fixed(_)
    ) {
        args.push("{address}".into());
    }
    for arg in &recipe.args {
        args.push(format!("{{{}}}", arg.name));
    }
    format!("{new}({})", args.join(", "))
}

fn escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_are_stable_sha256() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn a_missing_lock_verifies_and_records() {
        let entry = LockEntry {
            source: "drivers/demo.kdl".into(),
            manifest_sha256: sha256_hex(b"element"),
            ..LockEntry::default()
        };
        let used = [UsedDriver {
            tag: "demo".into(),
            manifest: PathBuf::from("drivers/demo.kdl"),
            entry: entry.clone(),
        }];
        assert!(verify(None, &used).is_ok());
        let lock = record(&used);
        assert_eq!(lock.drivers.get("demo"), Some(&entry));
        assert!(verify(Some(&lock), &used).is_ok());
    }

    #[test]
    fn a_changed_manifest_is_stale() {
        let entry = LockEntry {
            source: "drivers/demo.kdl".into(),
            manifest_sha256: sha256_hex(b"old"),
            ..LockEntry::default()
        };
        let used = [UsedDriver {
            tag: "demo".into(),
            manifest: PathBuf::from("drivers/demo.kdl"),
            entry: LockEntry {
                manifest_sha256: sha256_hex(b"new"),
                ..entry.clone()
            },
        }];
        let lock = DriversLock {
            version: LOCK_VERSION,
            drivers: [("demo".to_string(), entry)].into_iter().collect(),
        };
        let problems = verify(Some(&lock), &used).expect_err("stale");
        assert!(matches!(problems[0], LockProblem::Stale { .. }));
        assert!(problems[0].to_string().contains("DRIVER_LOCK_STALE"));
    }

    #[test]
    fn a_lock_without_the_entry_is_missing() {
        let used = [UsedDriver {
            tag: "demo".into(),
            manifest: PathBuf::from("drivers/demo.kdl"),
            entry: LockEntry {
                source: "x".into(),
                manifest_sha256: sha256_hex(b"x"),
                ..LockEntry::default()
            },
        }];
        let problems = verify(Some(&DriversLock::new()), &used).expect_err("missing");
        assert!(problems[0].to_string().contains("DRIVER_LOCK_MISSING"));
        assert!(problems[0].to_string().contains("driver add --local"));
    }

    #[test]
    fn a_v1_lock_reads_and_upgrades_in_memory() {
        let text = r#"{"version":1,"drivers":{"demo":{"source":"drivers/demo.kdl","manifest_sha256":"abc"}}}"#;
        let mut lock: DriversLock = serde_json::from_str(text).unwrap();
        for entry in lock.drivers.values_mut() {
            if entry.kind.is_empty() {
                entry.kind = "local".into();
            }
        }
        assert_eq!(lock.drivers["demo"].kind, "local");
    }

    #[test]
    fn migration_rewrites_the_bundled_v1_fixture() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("testing/fixtures/schema-v1/drivers.schema.kdl");
        let text = migrate(&path).expect("the fixture migrates");
        assert!(text.contains("driver \"ssd1306\""));
        assert!(text.contains("format_version 2"));
        assert!(!text.contains("device_recipe"));
        // The output is itself a valid v2 manifest.
        let schema = espforge_schema::load_all(&[
            espforge_schema::SCHEMA_KDL,
            espforge_schema::SERVICES_SCHEMA_KDL,
            &text,
        ])
        .expect("the migrated file loads");
        for tag in espforge_drivers::TAGS {
            let element = schema.element(tag).expect(tag);
            assert_eq!(element.format, espforge_schema::Format::V2);
            assert!(element.device_recipe.is_some());
        }
    }
}
