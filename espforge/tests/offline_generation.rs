//! `check`/`gen`/`build` are offline: a project with a local driver manifest
//! must generate even when every network path is pointed at a dead address.
//!
//! The driver lock lives beside the `.espf`, not in the disposable output:
//! deleting `build/` must not bypass lock verification.

use std::path::Path;
use std::process::Command;

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let path = entry.path();
        let target = to.join(entry.file_name());
        if path.is_dir() {
            copy_dir(&path, &target);
        } else {
            std::fs::copy(&path, &target).unwrap();
        }
    }
}

/// Generate `root/project.espf` into `out` with every network path dead.
fn generate_offline(root: &Path, out: &Path) {
    let output = Command::new(env!("CARGO_BIN_EXE_espforge"))
        .args([
            "gen",
            root.join("project.espf").to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .env("HTTPS_PROXY", "http://127.0.0.1:9")
        .env("HTTP_PROXY", "http://127.0.0.1:9")
        .env("ALL_PROXY", "http://127.0.0.1:9")
        .env("NO_PROXY", "")
        .output()
        .expect("the espforge binary runs");
    assert!(
        output.status.success(),
        "gen failed:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn generation_does_not_touch_the_network() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let project = tempfile::tempdir().unwrap();
    let root = project.path();

    std::fs::create_dir_all(root.join("drivers")).unwrap();
    std::fs::copy(
        repo.join("testing/fixtures/drivers/accel.kdl"),
        root.join("drivers/accel.kdl"),
    )
    .unwrap();
    copy_dir(
        &repo.join("testing/fixtures/fx-accel-driver"),
        &root.join("fx-accel-driver"),
    );
    copy_dir(
        &repo.join("testing/fixtures/local-driver/app"),
        &root.join("app"),
    );
    std::fs::copy(
        repo.join("testing/fixtures/local-driver/project.espf"),
        root.join("project.espf"),
    )
    .unwrap();

    let out = root.join("out");
    generate_offline(root, &out);
    let generated = std::fs::read_to_string(out.join("src/generated.rs")).unwrap();
    assert!(generated.contains("FxAccel"), "{generated}");

    // The lock is beside the spec, next to the project's `drivers/`.
    let lock_path = root.join(".espforge/drivers.lock");
    let lock = std::fs::read_to_string(&lock_path).unwrap();
    assert!(lock.contains("\"version\": 2"), "{lock}");
    assert!(!out.join(".espforge/drivers.lock").exists());

    // `build/` is disposable: deleting and regenerating it verifies the same
    // root lock rather than quietly bypassing it.
    std::fs::remove_dir_all(&out).unwrap();
    generate_offline(root, &out);
    assert_eq!(
        std::fs::read_to_string(&lock_path).unwrap(),
        lock,
        "the root lock is verified and rewritten unchanged"
    );
}
