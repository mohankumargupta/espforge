//! Two projects in one process: each run's schema belongs to that run.
//!
//! Every other test in this suite spawns `espforge`, so each project gets a
//! fresh process and a fresh schema. These two calls share a process on
//! purpose: project B must not be answered from project A's drivers.

use espforge_core::answers::Overrides;
use espforge_core::pipeline;

/// A v2 project driver with one tag.
///
/// Both fixtures name the same crate: what matters is that each run composes
/// its own manifest, not which crate the driver binds.
fn manifest(tag: &str, ty: &str) -> Vec<(String, String)> {
    let text = format!(
        r##"driver "{tag}" {{
    format_version 2
    summary "Fixture {tag}"
    category "sensor"
    dependency "fixture-crate" {{ path "crate" }}
    bus "i2c" {{ address fixed="0x10" }}
    codegen {{
        runtime "blocking" {{
            type "fixture_crate::{ty}"
            constructor "fixture_crate::{ty}::new({{bus_device}}, {{address}})"
        }}
    }}
    example "minimal" #"<{tag} id="{tag}" bus="$i2c0" address="0x10" />"#
}}
"##
    );
    vec![(format!("drivers/{tag}.kdl"), text)]
}

/// `testing/fixtures/local-driver/project.espf`'s shape, with `fx_accel`.
const SPEC_A: &str = "# espforge\n\
    \x20 <project name=\"a\" />\n\
    \x20 <chip type=\"esp32c3\" />\n\
    \x20 <runtime type=\"blocking\" />\n\
    # peripherals\n\
    \x20 <i2c id=\"i2c0\" sda=\"6\" scl=\"7\" />\n\
    # devices\n\
    \x20 <fx_accel id=\"accel\" bus=\"$i2c0\" address=\"0x10\" />\n\
    # services\n";

/// The same project, driving a different tag.
const SPEC_B: &str = "# espforge\n\
    \x20 <project name=\"b\" />\n\
    \x20 <chip type=\"esp32c3\" />\n\
    \x20 <runtime type=\"blocking\" />\n\
    # peripherals\n\
    \x20 <i2c id=\"i2c0\" sda=\"6\" scl=\"7\" />\n\
    # devices\n\
    \x20 <fx_screen id=\"screen\" bus=\"$i2c0\" address=\"0x10\" />\n\
    # services\n";

/// The regression: before sessions, `install` kept the first project's schema
/// and B failed with `DEVICE_UNSUPPORTED` for its own driver.
#[test]
fn two_projects_in_one_process_do_not_share_drivers() {
    let a = pipeline::validate_with(
        SPEC_A,
        &Overrides::default(),
        &manifest("fx_accel", "FxAccel"),
    )
    .expect("project A composes its own driver");
    let b = pipeline::validate_with(
        SPEC_B,
        &Overrides::default(),
        &manifest("fx_screen", "FxScreen"),
    )
    .expect("project B composes its own driver, not project A's");

    assert_eq!(a.board.devices.len(), 1);
    assert_eq!(b.board.devices.len(), 1);

    // Each validated result carries the schema it was checked against, and
    // neither one can see the other's driver.
    assert!(a.session.schema().element("fx_accel").is_some());
    assert!(a.session.schema().element("fx_screen").is_none());
    assert!(b.session.schema().element("fx_screen").is_some());
    assert!(b.session.schema().element("fx_accel").is_none());
}

/// The same seam from the other side: a spec naming a tag the run did not
/// compose is refused, whichever project ran first.
#[test]
fn a_tag_from_another_project_is_not_known() {
    assert!(
        pipeline::validate_with(
            SPEC_B,
            &Overrides::default(),
            &manifest("fx_accel", "FxAccel")
        )
        .is_err(),
        "project B must not accept project A's tag"
    );
    assert!(
        pipeline::validate_with(
            SPEC_A,
            &Overrides::default(),
            &manifest("fx_screen", "FxScreen")
        )
        .is_err(),
        "project A must not accept project B's tag"
    );
}
