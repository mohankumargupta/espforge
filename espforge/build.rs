// Resolves the exact version of the esp-generate dependency from the build
// graph and exports it as an env var readable at compile time via env!().
// Used by `espforge version`.

use cargo_metadata::MetadataCommand;

fn main() {
    println!("cargo:rerun-if-changed=Cargo.toml");

    // Full (non-`no_deps`) metadata is required here: only this mode lists the
    // dependency packages resolved from the workspace, not just workspace
    // members.
    let metadata = MetadataCommand::new()
        .exec()
        .expect("build.rs: `cargo metadata` should always be runnable inside a cargo build");

    let version = metadata
        .packages
        .iter()
        .find(|package| package.name == "espforge-espgenerate-template")
        .map(|package| package.version.to_string())
        .expect(
            "build.rs: espforge-espgenerate-template dependency must be part of the build graph",
        );

    println!("cargo:rustc-env=ESP_GENERATE_VERSION={version}");
}
