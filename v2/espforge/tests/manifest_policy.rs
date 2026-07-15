//! Integration tests for manifest-emission policy (design §19), focused on the
//! networking / `http` software-service component (ADR-012 / ADR-013). These
//! assert on the emitted `Cargo.toml` text — the unit under test for manifest
//! policy — using only the public `espforge::*` pipeline API.
//!
//! Under Option B (ADR-012) espforge does NOT author the esp-hal / esp-rtos /
//! embassy-net / esp-radio / edge stack. `esp-generate` (Layer 1 scaffold)
//! produces that base manifest; espforge only *merges* `espforge-runtime` (+ the
//! resolved module features) into it. The tests therefore seed a realistic
//! esp-generate-style base `Cargo.toml` in a temp `out_dir`, run the real
//! `emit` (which reads that base to perform the merge), and assert on the
//! merged result.

use std::path::Path;

use espforge::emit::rust;
use espforge::parse;
use espforge::pipeline;

/// A minimal stand-in for what `esp-generate` scaffolds: version-locked deps
/// for the chosen chip + options, including the network stack `http` needs.
const ESP_GENERATE_BASE: &str = r#"[package]
name = "espforge_project"
version = "0.1.0"
edition = "2021"

[dependencies]
esp-backtrace = { version = "0.15.1", features = ["esp32c3", "panic-handler"] }
esp-hal = { version = "1.1.0", features = ["esp32c3", "unstable"] }
esp-println = { version = "0.13.0", features = ["esp32c3"] }
esp-radio = { version = "1.0.0", features = ["esp32c3", "wifi"] }
embassy-executor = { version = "0.9.0", features = ["executor-thread", "integrated-timers"] }
embassy-net = { version = "0.9.0", features = ["tcp", "dhcpv4", "medium-ethernet"] }
embassy-time = { version = "0.5.0", features = ["generic-queue-64"] }
esp-rtos = "0.3.0"
log = "0.4.27"
static-cell = "1.0.0"
"#;

/// Run the full pipeline (parse -> validate -> resolve -> emit) on an example
/// spec, seeding a synthetic esp-generate base `Cargo.toml` in a temp `out_dir`
/// so the Option-B merge has something to merge into. Returns the merged
/// `Cargo.toml` text.
fn emitted_cargo_toml(spec_rel: &str) -> String {
    let v2 = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let spec = Path::new(&v2)
        .join("../espforge-examples/examples")
        .join(spec_rel);
    let project = parse::parse_file(&spec)
        .unwrap_or_else(|e| panic!("parse {}: {e:?}", spec.display()));
    pipeline::validate(&project).expect("validate");
    let ir = pipeline::resolve(&project);

    let out = std::env::temp_dir().join(format!(
        "espforge_mp_{}_{}",
        std::process::id(),
        sanitize_spec(spec_rel)
    ));
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(out.join("Cargo.toml"), ESP_GENERATE_BASE).unwrap();

    let artifacts = rust::emit(&ir, &out).expect("emit");
    artifacts
        .into_iter()
        .find(|a| a.path == "Cargo.toml")
        .map(|a| a.content)
        .expect("Cargo.toml artifact")
}

fn sanitize_spec(s: &str) -> String {
    s.chars().map(|c| if c.is_alphanumeric() { c } else { '_' }).collect()
}

/// `http` (a software-service component, ADR-012) merges in the
/// `espforge-runtime` dep with its `http` feature, and forces Embassy even when
/// the YAML says `runtime: blocking` (auto-upgrade, Q1/Q12). The network stack
/// deps (`esp-radio`, `embassy-net`, `embassy-executor`) stay owned by the
/// esp-generate base — espforge never re-authors them.
#[test]
fn http_requests_network_deps_and_http_feature() {
    let cargo = emitted_cargo_toml("05.Networking/wifi/http_example/http_example.yaml");
    // The esp-generate base owns the WiFi + network stack deps and they must
    // survive the merge untouched.
    assert!(
        cargo.contains("esp-radio = "),
        "esp-generate base (esp-radio) must survive the merge:\n{cargo}"
    );
    assert!(
        !cargo.contains("esp-wifi = "),
        "http example must NOT use the older esp-wifi crate:\n{cargo}"
    );
    assert!(
        cargo.contains("embassy-net = "),
        "esp-generate base (embassy-net) must survive the merge:\n{cargo}"
    );
    assert!(
        cargo.contains("embassy-executor = "),
        "http must force Embassy (embassy-executor dep missing):\n{cargo}"
    );
    // Under Option B, edge-http / edge-nal / edge-nal-embassy are transitive
    // through espforge-runtime's feature graph — the project must NOT name them
    // directly.
    assert!(
        !cargo.contains("edge-http = "),
        "edge-http must be transitive via espforge-runtime, not a direct dep:\n{cargo}"
    );
    assert!(
        !cargo.contains("edge-nal = "),
        "edge-nal must be transitive via espforge-runtime, not a direct dep:\n{cargo}"
    );
    // The runtime `http` feature is requested on espforge-runtime.
    assert!(
        cargo.contains("espforge-runtime")
            && (cargo.contains("features = [ \"http\" ]")
                || cargo.contains("\"http\"")),
        "http example must request the espforge-runtime http feature:\n{cargo}"
    );
}

/// A `http` component without a top-level `esp32.wifi` block must fail
/// validation (ADR-012 §8).
#[test]
fn http_without_wifi_block_fails_validation() {
    let spec = r#"
espforge:
  name: broken_http
  target: esp32c3
  runtime: embassy
esp32: {}
components:
  web:
    using: http
devices: []
"#;
    let dir = std::env::temp_dir().join("espforge_broken_http_test.yaml");
    std::fs::write(&dir, spec).unwrap();
    let project = parse::parse_file(&dir).expect("parse");
    let res = pipeline::validate(&project);
    let _ = std::fs::remove_file(&dir);
    assert!(
        res.is_err(),
        "http without esp32.wifi must fail validation, got Ok"
    );
    let diags = res.unwrap_err();
    assert!(
        diags.iter().any(|d| d.message.contains("esp32.wifi")),
        "validation error should mention the missing esp32.wifi block: {diags:?}"
    );
}
