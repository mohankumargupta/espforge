//! End-to-end tests for `driver add`'s fetched sources.
//!
//! Every fixture is local (`file://` with `--allow-local-source`); no test
//! touches a remote host. The cache is the real user cache because the CLI's
//! cache path is process-wide, so each test uses a unique tag and removes it.

use std::path::Path;
use std::process::Command;

use espforge_core::drivers;

fn unique(prefix: &str) -> String {
    format!(
        "{prefix}_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    )
}

fn manifest(tag: &str) -> String {
    format!(
        r##"driver "{tag}" {{
    format_version 2
    summary "Fetched fixture"
    category "sensor"
    dependency "fixture-crate" {{ path "crate" }}
    bus "i2c" {{ address fixed="0x10" }}
    codegen {{
        runtime "blocking" {{
            type "fixture_crate::Fixture"
            constructor "fixture_crate::Fixture::new({{bus_device}}, {{address}})"
        }}
    }}
    example "minimal" #"<{tag} id="fixture" bus="$i2c0" address="0x10" />"#
}}
"##
    )
}

fn write_package(root: &Path, tag: &str) {
    std::fs::create_dir_all(root.join("crate/src")).unwrap();
    std::fs::write(root.join("driver.kdl"), manifest(tag)).unwrap();
    std::fs::write(
        root.join("crate/Cargo.toml"),
        "[package]\nname = \"fixture-crate\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::write(root.join("crate/src/lib.rs"), "pub struct Fixture;\n").unwrap();
}

fn tar_gz(root: &Path) -> Vec<u8> {
    let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    let mut builder = tar::Builder::new(encoder);
    builder.append_dir_all(".", root).unwrap();
    builder.into_inner().unwrap().finish().unwrap()
}

fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".espforge")).unwrap();
    dir
}

fn cleanup(project: &Path, tag: &str) {
    let _ = drivers::remove(project, tag, true);
    if let Some(root) = drivers::cache_root() {
        let _ = std::fs::remove_dir_all(root.join(tag));
    }
}

#[test]
fn an_archive_lands_in_the_cache_and_locks_both_hashes() {
    let tag = unique("fx_https");
    let source = tempfile::tempdir().unwrap();
    write_package(source.path(), &tag);
    let archive = tempfile::tempdir().unwrap();
    let path = archive.path().join("fx.tar.gz");
    std::fs::write(&path, tar_gz(source.path())).unwrap();

    let project = project();
    let url = format!("file://{}", path.display());
    let line = drivers::add_spec(
        project.path(),
        &url,
        None,
        None,
        true,
        false,
        None,
        Some("1.2.3"),
        false,
    )
    .expect("the archive adds");
    assert!(line.contains(&format!("<{tag}>")), "{line}");

    let lock = drivers::read_lock(project.path()).unwrap().unwrap();
    assert_eq!(lock.version, 2);
    let entry = &lock.drivers[&tag];
    assert_eq!(entry.kind, "https");
    assert_eq!(entry.requested.as_deref(), Some(url.as_str()));
    assert!(entry.manifest_sha256.len() == 64);
    assert!(entry.payload_sha256.is_some());
    let cache = entry.cache_path.as_ref().expect("cache path");
    assert!(cache.is_file(), "{}", cache.display());
    assert!(cache.with_file_name("crate/Cargo.toml").is_file());

    assert!(drivers::verify_cache(project.path(), false, true).is_ok());
    cleanup(project.path(), &tag);
}

#[test]
fn a_checksum_mismatch_is_refused() {
    let tag = unique("fx_sha");
    let source = tempfile::tempdir().unwrap();
    write_package(source.path(), &tag);
    let archive = tempfile::tempdir().unwrap();
    let path = archive.path().join("fx.tar.gz");
    std::fs::write(&path, tar_gz(source.path())).unwrap();

    let project = project();
    let url = format!("file://{}", path.display());
    let error = drivers::add_spec(
        project.path(),
        &url,
        None,
        None,
        true,
        false,
        Some(&"0".repeat(64)),
        None,
        false,
    )
    .unwrap_err();
    assert!(error.contains("DRIVER_CHECKSUM_MISMATCH"), "{error}");
    cleanup(project.path(), &tag);
}

#[test]
fn a_tampered_cache_entry_is_stale() {
    let tag = unique("fx_tamper");
    let source = tempfile::tempdir().unwrap();
    write_package(source.path(), &tag);
    let archive = tempfile::tempdir().unwrap();
    let path = archive.path().join("fx.tar.gz");
    std::fs::write(&path, tar_gz(source.path())).unwrap();

    let project = project();
    let url = format!("file://{}", path.display());
    drivers::add_spec(
        project.path(),
        &url,
        None,
        None,
        true,
        false,
        None,
        None,
        false,
    )
    .unwrap();
    let lock = drivers::read_lock(project.path()).unwrap().unwrap();
    let cache = lock.drivers[&tag].cache_path.clone().unwrap();
    std::fs::write(
        &cache,
        format!("// tampered\n{}", std::fs::read_to_string(&cache).unwrap()),
    )
    .unwrap();
    let error = drivers::verify_cache(project.path(), false, true).unwrap_err();
    assert!(error.contains("DRIVER_LOCK_STALE"), "{error}");
    cleanup(project.path(), &tag);
}

#[test]
fn a_pinned_git_revision_lands_in_the_cache() {
    if Command::new("git").arg("--version").output().is_err() {
        eprintln!("skipping: git is not installed");
        return;
    }
    let tag = unique("fx_git");
    let repo = tempfile::tempdir().unwrap();
    write_package(repo.path(), &tag);
    let git = |args: &[&str]| {
        let status = Command::new("git")
            .args(args)
            .current_dir(repo.path())
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    };
    git(&["init", "--quiet"]);
    git(&["add", "."]);
    git(&["commit", "--quiet", "-m", "fixture"]);
    let commit = String::from_utf8(
        Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(repo.path())
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap()
    .trim()
    .to_string();

    let project = project();
    let url = format!("git+file://{}#{commit}", repo.path().display());
    let line = drivers::add_spec(
        project.path(),
        &url,
        None,
        None,
        true,
        false,
        None,
        None,
        false,
    )
    .expect("the git package adds");
    assert!(line.contains(&format!("<{tag}>")), "{line}");
    let lock = drivers::read_lock(project.path()).unwrap().unwrap();
    let entry = &lock.drivers[&tag];
    assert_eq!(entry.kind, "git");
    assert_eq!(entry.commit.as_deref(), Some(commit.as_str()));
    assert!(entry.cache_path.as_ref().unwrap().is_file());
    cleanup(project.path(), &tag);
}

#[test]
fn a_registry_index_resolves_an_exact_version() {
    let tag = unique("fx_index");
    let source = tempfile::tempdir().unwrap();
    write_package(source.path(), &tag);
    let archive = tempfile::tempdir().unwrap();
    let path = archive.path().join("fx.tar.gz");
    let bytes = tar_gz(source.path());
    std::fs::write(&path, &bytes).unwrap();

    let index = tempfile::tempdir().unwrap();
    let index_path = index.path().join("index.json");
    let document = serde_json::json!({
        "schema": 1,
        "drivers": {
            tag.clone(): [{
                "version": "1.0.0",
                "url": format!("file://{}", path.display()),
                "sha256": drivers::sha256_hex(&bytes),
            }]
        }
    });
    std::fs::write(&index_path, serde_json::to_vec_pretty(&document).unwrap()).unwrap();

    let project = project();
    let url = format!("file://{}", index_path.display());
    let line = drivers::add_spec(
        project.path(),
        &format!("{tag}@1.0.0"),
        None,
        Some(&url),
        true,
        false,
        None,
        None,
        false,
    )
    .expect("the registry package adds");
    assert!(line.contains(&format!("<{tag}>")), "{line}");
    let lock = drivers::read_lock(project.path()).unwrap().unwrap();
    assert_eq!(lock.drivers[&tag].kind, "registry");
    assert_eq!(
        lock.drivers[&tag].requested.as_deref(),
        Some(format!("{tag}@1.0.0").as_str())
    );
    cleanup(project.path(), &tag);
}

#[test]
fn a_v1_lock_is_upgraded_when_a_source_is_added() {
    let project = project();
    std::fs::create_dir_all(project.path().join(".espforge")).unwrap();
    std::fs::write(
        drivers::lock_path(project.path()),
        r#"{"version":1,"drivers":{"old":{"source":"old.kdl","manifest_sha256":"abc"}}}"#,
    )
    .unwrap();
    let tag = unique("fx_upgrade");
    let source = tempfile::tempdir().unwrap();
    write_package(source.path(), &tag);
    let archive = tempfile::tempdir().unwrap();
    let path = archive.path().join("fx.tar.gz");
    std::fs::write(&path, tar_gz(source.path())).unwrap();
    drivers::add_spec(
        project.path(),
        &format!("file://{}", path.display()),
        None,
        None,
        true,
        false,
        None,
        None,
        false,
    )
    .unwrap();
    let lock = drivers::read_lock(project.path()).unwrap().unwrap();
    assert_eq!(lock.version, 2);
    assert_eq!(lock.drivers["old"].kind, "local");
    assert!(lock.drivers.contains_key(&tag));
    cleanup(project.path(), &tag);
}
