//! `<i2c>` peripheral + `<i2c_component>` lowering (step 1d).
//!
//! Moved verbatim from `espforge/src/commands/pipeline.rs::hardware`: the
//! `i2c_peripheral_name` helper, the `<i2c>` peripheral branch, and the
//! `<i2c_component>` component branch.

use super::util::parse_khz;
use super::I2cSpec;
use espf::context::{ParsedNode, RuntimeContext};
use espf::diag::EspfError;
use espforge_bindings::components::I2cComponent;
use espforge_model::hardware::is_rust_ident;

/// One `<i2c>` peripheral plus the component riding on it (if any).
#[derive(Debug, Clone)]
pub struct I2cBusDraft {
    pub id: String,
    pub peripheral: String,
    pub sda: u8,
    pub scl: u8,
    pub khz: u32,
    /// The component id riding on this bus, if any.
    pub used: Option<String>,
}

/// `i2c0` -> `I2C0`, `i2c1` -> `I2C1`, `i2c2` -> `I2C2`; anything else is the
/// user's error (the esp-hal peripheral structs are named exactly this).
fn i2c_peripheral_name(n: &ParsedNode) -> Result<String, EspfError> {
    let id = n.id;
    if id == "i2c0" || id == "i2c1" || id == "i2c2" {
        return Ok(id.to_ascii_uppercase());
    }
    Err(EspfError::at_node(
        n,
        "invalid i2c id",
        format!("<i2c id=\"{id}\">: the id must be i2c0, i2c1 or i2c2"),
    ))
}

/// The `<i2c>` branch of the `# peripherals` loop.
pub fn lower_i2c(n: &ParsedNode) -> Result<I2cBusDraft, EspfError> {
    let pin_attr = |what: &str| -> Result<u8, EspfError> {
        n.get_attr(what)
            .and_then(|p| p.parse::<u8>().ok())
            .ok_or_else(|| {
                EspfError::at_node(
                    n,
                    "invalid i2c pin",
                    format!("<i2c id=\"{}\"> needs a numeric {what}=\"...\"", n.id),
                )
            })
    };
    let khz = match n.get_attr("frequency") {
        None => 100, // esp-hal's default
        Some(f) => parse_khz(f, "i2c", n.id)
            .map_err(|m| EspfError::at_node(n, "invalid i2c frequency", m))?,
    };
    Ok(I2cBusDraft {
        id: n.id.to_string(),
        peripheral: i2c_peripheral_name(n)?,
        sda: pin_attr("sda")?,
        scl: pin_attr("scl")?,
        khz,
        used: None,
    })
}

/// The `<i2c_component>` branch of the `# components` loop.
///
/// Step 2c (pilot): the `$` parent lookup goes through the typed wrapper
/// (`I2cComponent::resolve`), the sole consumer of the context handles.
/// Bus occupancy (`both use`) and the slug check stay here until 2d/2e;
/// the slug check duplicates the wrapper so the message stays byte-identical.
pub fn attach_i2c_component(
    n: &ParsedNode,
    i2c_buses: &mut [I2cBusDraft],
    ctx: &RuntimeContext,
) -> Result<(), EspfError> {
    // Single dispatch path for the `$` ref; wrapper owns `trim_start_matches`.
    // Wrapper `String` errors are located at this node so `Invalid::report`
    // renders them as snippets with identical text.
    let comp =
        I2cComponent::resolve(n, ctx).map_err(|m| EspfError::at_node(n, "invalid i2c_component", m))?;
    // Locate the draft via the resolved handle, not a string `find`.
    let idx = i2c_buses
        .iter()
        .position(|b| ctx.interner.get(&b.id) == Some(comp.peripheral_handle))
        .ok_or_else(|| {
            let target = comp.i2c_ref.trim_start_matches('$');
            EspfError::at_node(
                n,
                "unknown i2c bus",
                format!(
                    "<i2c_component id=\"{}\"> refers to `{target}`, which is not an <i2c> in `# peripherals`",
                    n.id
                ),
            )
        })?;
    let target = i2c_buses[idx].id.clone();
    if let Some(other) = i2c_buses[idx].used.as_deref() {
        return Err(EspfError::at_node(
            n,
            "i2c bus already used",
            format!(
                "<i2c_component id=\"{other}\"> and <i2c_component id=\"{}\"> both use <i2c id=\"{target}\">",
                n.id
            ),
        ));
    }
    let name = n.id.replace('-', "_");
    if !is_rust_ident(&name) {
        return Err(EspfError::at_node(
            n,
            "invalid component id",
            format!(
                "<i2c_component id=\"{}\">: the id must become a Rust identifier (letters, digits, `_`; not starting with a digit)",
                n.id
            ),
        ));
    }
    i2c_buses[idx].used = Some(name);
    Ok(())
}

/// A bare `<i2c>` with no component generates nothing: the app can only
/// reach the bus through a component anyway.
pub fn finish_i2c(buses: Vec<I2cBusDraft>) -> Vec<I2cSpec> {
    buses
        .into_iter()
        .filter_map(|b| {
            b.used.map(|name| I2cSpec {
                name,
                peripheral: b.peripheral,
                sda: b.sda,
                scl: b.scl,
                khz: b.khz,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::super::lower_sections;
    use super::*;

    const WITH_BUS: &str = "# espforge\n  <project name=\"t\" />\n  <chip type=\"esp32c3\" />\n  <runtime type=\"blocking\" />\n# peripherals\n  <i2c id=\"i2c0\" sda=\"4\" scl=\"5\" />\n# components\n  <i2c_component id=\"c\" i2c=\"$i2c0\" address=\"0x48\" />\n# devices\n# services\n";

    const I2C_400K: &str = "# espforge\n  <project name=\"t\" />\n  <chip type=\"esp32c3\" />\n  <runtime type=\"blocking\" />\n# peripherals\n  <i2c id=\"i2c0\" sda=\"4\" scl=\"5\" frequency=\"400kHz\" />\n# components\n  <i2c_component id=\"c\" i2c=\"$i2c0\" address=\"0x48\" />\n# devices\n# services\n";

    fn sections(src: &str) -> std::collections::HashMap<String, Vec<espf::context::ParsedNode>> {
        espf::parser::parse_espf(src).sections
    }

    fn lower_src(src: &str) -> Result<super::super::Hardware, String> {
        lower_sections(&sections(src))
    }

    #[test]
    fn an_i2c_bus_with_a_component_becomes_hardware() {
        let hw = lower_src(WITH_BUS).unwrap();
        assert_eq!(
            hw.i2cs,
            [I2cSpec {
                name: "c".into(),
                peripheral: "I2C0".into(),
                sda: 4,
                scl: 5,
                khz: 100,
            }]
        );
    }

    #[test]
    fn i2c_frequency_is_parsed() {
        let spec = &lower_src(I2C_400K).unwrap().i2cs[0];
        assert_eq!(spec.khz, 400);

        let mhz = I2C_400K.replace("400kHz", "400MHZ");
        let err = lower_src(&mhz).unwrap_err();
        assert!(err.contains("not supported yet"), "{err}");
    }

    #[test]
    fn an_i2c_component_needs_a_declared_i2c() {
        // The ref passes graph resolution but the component then points at a
        // gpio, not an `<i2c>`. Step 2c pilot: the `$` lookup goes through
        // `I2cComponent::resolve`, so the wrapper's message replaces
        // hardware's old "not an <i2c>" line.
        let src = WITH_BUS
            .replace("<i2c id=", "<gpio id=")
            .replace(" sda=\"4\" scl=\"5\"", " pin=\"4\"");
        let err = lower_src(&src).unwrap_err();
        assert!(err.contains("expects an i2c"), "{err}");
    }

    #[test]
    fn two_i2c_components_on_one_bus_are_refused() {
        let src = WITH_BUS.replace(
            "# devices",
            "  <i2c_component id=\"c2\" i2c=\"$i2c0\" address=\"0x49\" />\n# devices",
        );
        let err = lower_src(&src).unwrap_err();
        assert!(
            err.contains("<i2c_component id=\"c\">") && err.contains("both use"),
            "{err}"
        );
    }

    #[test]
    fn a_gpio_sharing_a_pin_with_i2c_is_refused() {
        let src = WITH_BUS.replace(
            "# components",
            "  <gpio id=\"gpio5\" pin=\"5\" direction=\"output\" />\n# components",
        );
        let err = lower_src(&src).unwrap_err();
        assert!(err.contains("both use GPIO5"), "{err}");
    }

    #[test]
    fn a_bare_i2c_without_a_component_generates_nothing() {
        let src = WITH_BUS.replace(
            "# components\n  <i2c_component id=\"c\" i2c=\"$i2c0\" address=\"0x48\" />\n",
            "# components\n",
        );
        assert!(lower_src(&src).unwrap().i2cs.is_empty());
    }

    #[test]
    fn i2c_ids_must_be_i2c0_to_i2c2() {
        let src = WITH_BUS
            .replace("<i2c id=\"i2c0\"", "<i2c id=\"bus0\"")
            .replace("i2c=\"$i2c0\"", "i2c=\"$bus0\"");
        let err = lower_src(&src).unwrap_err();
        assert!(err.contains("i2c0, i2c1 or i2c2"), "{err}");
    }
}
