//! Typed wrapper for the `<spi_component>` bus component.
//!
//!   `<spi_component id="display_bus" spi="$spi2" mode="0" cs="gpio5" frequency="40MHz" />`
//!
//! Presence comes from `espf.schema.kdl`, generated into
//! [`schema::Attrs`](crate::components::generated_elements::spi_component).
//! The schema records `mode` as `default="0"` with `range="0..=3"`, which is
//! what the SPI examples and the wrapper agreed on (the examples keep `mode`
//! on `<spi>`; the component defaults it). `cs` is `kind="pin_or_ref"`: a
//! numeric pin like `cs="9"` or a gpio id like `cs="gpio5"`.
//!
//! This module owns the cs/reference lookup and the wording of the errors.

use espf::context::{ActivePeripheral, Handle, ParsedNode, RuntimeContext};

use crate::components::attrs::{Issue, PinOrRef};
use crate::components::generated_elements::spi_component as schema;

/// A fully validated `<spi_component>` node.
#[derive(Debug, Clone)]
pub struct SpiComponent<'a> {
    /// Value of the `id` attribute.
    pub id: &'a str,
    /// Raw `$`-reference string from the `spi` attribute (e.g. `"$spi2"`).
    pub spi_ref: &'a str,
    /// Parsed `mode` attribute (0..=3).
    pub mode: u32,
    /// The chip-select: a GPIO number, or a reference to the `<gpio>` that
    /// owns it.
    pub cs: PinOrRef<'a>,
    /// Raw `frequency` attribute, if supplied.
    pub frequency: Option<&'a str>,
    /// Handle of the referenced parent spi peripheral.
    pub peripheral_handle: Handle,
    /// The resolved parent peripheral (must be `ActivePeripheral::Spi`).
    pub peripheral: ActivePeripheral,
}

impl<'a> SpiComponent<'a> {
    /// Validates and resolves a spi_component node against the runtime context.
    pub fn resolve(node: &'a ParsedNode<'a>, ctx: &RuntimeContext<'a>) -> Result<Self, String> {
        let attrs = schema::Attrs::from_node(node).map_err(|issue| message(node.id, issue))?;
        let id = attrs.id.value;
        // Numeric `cs="9"` (the `spi_example`s) is a pin number, not a gpio
        // id — accept it without an interner lookup. Otherwise it must point
        // at a known gpio peripheral.
        if let PinOrRef::Ref(reference) = attrs.cs {
            ctx.interner.get(reference.name).ok_or_else(|| {
                format!(
                    "❌ [spi_component] '{id}' cs references undefined gpio '{}'",
                    reference.name
                )
            })?;
        }

        let name = attrs.spi.name;
        let handle = ctx.interner.get(name).ok_or_else(|| {
            format!("❌ [spi_component] '{id}' references undefined spi peripheral '{name}'")
        })?;
        let peripheral = ctx.active_peripherals[handle.0 as usize]
            .as_ref()
            .ok_or_else(|| {
                format!("❌ [spi_component] '{id}' spi peripheral '{name}' is not active")
            })?;

        if !matches!(peripheral, ActivePeripheral::Spi) {
            return Err(format!(
                "❌ [spi_component] '{id}' expects an spi peripheral, got a non-SPI parent bus"
            ));
        }

        Ok(Self {
            id,
            spi_ref: attrs.spi.raw,
            mode: attrs.mode.value,
            cs: attrs.cs,
            frequency: attrs.frequency.map(|frequency| frequency.value),
            peripheral_handle: handle,
            peripheral: *peripheral,
        })
    }
}

/// This element's wording for a schema-level problem. `mode` keeps its
/// "expected 0..=3", which is the range the schema records.
fn message(id: &str, issue: Issue<'_>) -> String {
    match issue {
        Issue::Missing { attr } if id.is_empty() => {
            format!("❌ [spi_component] Missing required attribute '{attr}'")
        }
        Issue::Missing { attr } => {
            format!("❌ [spi_component] '{id}' missing required attribute '{attr}'")
        }
        Issue::Invalid { attr, value } => {
            format!("❌ [spi_component] '{id}' invalid {attr} '{value}': expected 0..=3")
        }
    }
}
