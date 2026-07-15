//! Integration tests for manifest-emission policy (design §19), focused on the
//! networking / `http` software-service component (ADR-012 / ADR-013). These
//! assert on the emitted `Cargo.toml` text — the unit under test for manifest
//! policy — using only the public `espforge::*` pipeline API.

use std::path::Path;

use espforge::emit::rust;
use espforge::parse;
use espforge::pipeline;

/// Run the full pipeline (parse -> validate -> resolve -> emit) on an example
/// spec and return the emitted `Cargo.toml` text.
fn emitted_cargo_toml(spec_rel: &str) -> String {
    let v2 = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let spec = Path::new(&v2)
        .join("../espforge-examples/examples")
        .join(spec_rel);
    let project = parse::parse_file(&spec)
        .unwrap_or_else(|e| panic!("parse {}: {e:?}", spec.display()));
    pipeline::validate(&project).expect("validate");
    let ir = pipeline::resolve(&project);
    let artifacts = rust::emit(&ir).expect("emit");
    artifacts
        .into_iter()
        .find(|a| a.path == "Cargo.toml")
        .map(|a| a.content)
        .expect("Cargo.toml artifact")
}

/// `http` (a software-service component, ADR-012) pulls the network stack deps
/// + the `espforge-runtime/http` feature, and forces Embassy even when the YAML
/// says `runtime: blocking` (auto-upgrade, Q1/Q12).
#[test]
fn http_requests_network_deps_and_http_feature() {
    let cargo = emitted_cargo_toml("05.Networking/wifi/http_example/http_example.yaml");
    // WiFi crate is `esp-radio` (not the older `esp-wifi`, ADR-012).
    assert!(
        cargo.contains("esp-radio = "),
        "http example must depend on esp-radio:\n{cargo}"
    );
    assert!(
        !cargo.contains("esp-wifi = "),
        "http example must NOT use the older esp-wifi crate:\n{cargo}"
    );
    // Network stack + edge HTTP stack.
    assert!(
        cargo.contains("embassy-net = "),
        "http example must depend on embassy-net:\n{cargo}"
    );
    assert!(
        cargo.contains("edge-http = "),
        "http example must depend on edge-http:\n{cargo}"
    );
    // The runtime `http` feature is requested.
    assert!(
        cargo.contains("\"http\"") || cargo.contains("features = [ \"http\" ]"),
        "http example must request the espforge-runtime http feature:\n{cargo}"
    );
    // `is_embassy` (asserted by the `http` driver) must have forced Embassy, so
    // the emitted runtime dep uses the embassy executor feature, not a blocking
    // main. We check the manifest doesn't keep the example on a blocking path
    // by asserting the embassy-executor dependency is present.
    assert!(
        cargo.contains("embassy-executor = "),
        "http must force Embassy (embassy-executor dep missing):\n{cargo}"
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
