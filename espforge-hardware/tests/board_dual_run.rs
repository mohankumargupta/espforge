//! Dual-run guard for Phase 2 increment 3.
//!
//! The old `lower` and the new `board::build` are both run over every valid
//! embedded example. Where the old path succeeds, the new path must succeed
//! and its `Board::hardware()` projection must equal the old `Hardware`
//! exactly; where the old path fails, the new path must fail too. This is the
//! migration's proof that the IR can represent everything the emitter needs
//! before increment 4 deletes the drafts.

use espf::context::RuntimeContext;
use espf::diag::EspfError;
use espf::parser::{Parsed, parse_espf};
use espforge_hardware::board::build;
use espforge_hardware::lower_spans;
use espforge_model::{Board, Console, Meta, Runtime};

fn meta() -> Meta {
    Meta {
        name: "dual-run".into(),
        description: None,
        chip: "esp32c3".into(),
        runtime: Runtime::Blocking,
        console: Console::Uart,
    }
}

fn build_from(parsed: &Parsed<'_>) -> Result<Board, Vec<EspfError>> {
    let mut ctx = RuntimeContext::new();
    ctx.resolve_and_bind(
        &parsed.sections["peripherals"],
        &parsed.sections["components"],
        &parsed.sections["devices"],
    )?;
    build(&parsed.sections, &ctx, &meta())
}

fn is_structurally_valid(parsed: &Parsed<'_>) -> bool {
    parsed.junk.is_empty() && parsed.structure_errors.is_empty()
}

#[test]
fn every_valid_example_projects_to_the_same_hardware() {
    let mut compared = 0;
    let mut both_failed = Vec::new();

    for category in espforge_examples::categories() {
        for example in &category.examples {
            let parsed = parse_espf(&example.content);
            if !is_structurally_valid(&parsed) {
                continue;
            }

            let name = format!("{}/{}", category.name, example.name);
            let old = lower_spans(&parsed.sections);
            let new = build_from(&parsed);

            match (old, new) {
                (Ok(old), Ok(new)) => {
                    assert_eq!(new.hardware(), old, "projection differs for {name}");
                    compared += 1;
                }
                (Err(old), Err(new)) => both_failed.push((name, old, new)),
                (Ok(_), Err(errs)) => {
                    panic!("new path failed where the old path succeeded for {name}: {errs:?}")
                }
                (Err(errs), Ok(_)) => {
                    panic!("new path succeeded where the old path failed for {name}: {errs:?}")
                }
            }
        }
    }

    assert!(
        compared >= 6,
        "expected several examples to compare, got {compared}"
    );

    // Known stale corpus: these cannot generate today. Both paths must agree
    // that they fail; the phase doc tracks them as decision D7.
    for (name, old, new) in both_failed {
        assert!(
            !old.is_empty() && !new.is_empty(),
            "{name} failed without a diagnostic"
        );
    }
}

#[test]
fn the_ir_path_rejects_the_same_hardware_as_the_old_path() {
    // A few specs the old path refuses on purpose: a pin conflict, a missing
    // required attribute, an unsupported element. The assertion here is only
    // "both refuse"; increment 4 makes the wording and primary span identical.
    let cases = [
        (
            "pin conflict",
            "# espforge\n  <project name=\"t\" />\n  <chip type=\"esp32c3\" />\n  <runtime type=\"blocking\" />\n# peripherals\n  <gpio id=\"g\" pin=\"4\" />\n  <i2c id=\"i2c0\" sda=\"4\" scl=\"5\" />\n# components\n# devices\n# services\n",
        ),
        (
            "missing address",
            "# espforge\n  <project name=\"t\" />\n  <chip type=\"esp32c3\" />\n  <runtime type=\"blocking\" />\n# peripherals\n  <i2c id=\"i2c0\" sda=\"4\" scl=\"5\" />\n# components\n  <i2c_component id=\"c\" i2c=\"$i2c0\" />\n# devices\n# services\n",
        ),
        (
            "unsupported peripheral",
            "# espforge\n  <project name=\"t\" />\n  <chip type=\"esp32c3\" />\n  <runtime type=\"blocking\" />\n# peripherals\n  <pwm id=\"pwm0\" />\n# components\n# devices\n# services\n",
        ),
    ];

    for (what, src) in cases {
        let parsed = parse_espf(src);
        assert!(
            lower_spans(&parsed.sections).is_err(),
            "old accepted {what}"
        );
        assert!(build_from(&parsed).is_err(), "new accepted {what}");
    }
}
