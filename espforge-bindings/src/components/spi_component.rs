//! Typed wrapper for the `<spi_component>` bus component.
//!
//! Schema (from `examples/example.espf.txt`):
//!   `<spi_component id="display_bus" spi="$spi2" mode="0" cs="gpio5" frequency="40MHz" />`
//!
//! Required attributes: `id`, `spi` (a `$`-prefixed reference to an `<spi>`
//! peripheral), `cs` (chip-select: either a numeric pin like `cs="9"` as in
//! the `spi_example`s, or a gpio id like `cs="gpio5"` as in
//! `examples/example.espf.txt`). `mode` (0..=3) and `frequency` are optional
//! on the component — the `spi_example`s keep both on `<spi>` — and default
//! to `0` / unset when omitted (see `espf.schema.kdl` TODO 11.11).

use espf::context::{ActivePeripheral, Handle, RuntimeContext};

/// A fully validated `<spi_component>` node.
#[derive(Debug, Clone)]
pub struct SpiComponent<'a> {
    /// Value of the `id` attribute.
    pub id: &'a str,
    /// Raw `$`-reference string from the `spi` attribute (e.g. `"$spi2"`).
    pub spi_ref: &'a str,
    /// Parsed `mode` attribute (0..=3).
    pub mode: u8,
    /// Raw chip-select reference string from the `cs` attribute — a plain
    /// gpio id, no `$` prefix (a legacy `$`-prefixed value is tolerated).
    pub cs_ref: &'a str,
    /// Raw `frequency` attribute, if supplied.
    pub frequency: Option<&'a str>,
    /// Handle of the referenced parent spi peripheral.
    pub peripheral_handle: Handle,
    /// The resolved parent peripheral (must be `ActivePeripheral::Spi`).
    pub peripheral: ActivePeripheral,
}

impl<'a> SpiComponent<'a> {
    /// Validates and resolves a spi_component node against the runtime context.
    pub fn resolve(node: &'a espf::context::ParsedNode<'a>, ctx: &RuntimeContext<'a>) -> Result<Self, String> {
        let id = node.id;
        if id.is_empty() {
            return Err("❌ [spi_component] Missing required attribute 'id'".to_string());
        }

        let spi_ref = node
            .get_attr("spi")
            .ok_or_else(|| format!("❌ [spi_component] '{id}' missing required attribute 'spi'"))?;

        // Optional on the component (examples keep it on `<spi>`); defaults to 0.
        let mode: u8 = match node.get_attr("mode") {
            None => 0,
            Some(mode_raw) => {
                let m: u8 = mode_raw.parse().map_err(|_| {
                    format!("❌ [spi_component] '{id}' invalid mode '{mode_raw}': expected 0..=3")
                })?;
                if m > 3 {
                    return Err(format!(
                        "❌ [spi_component] '{id}' invalid mode '{mode_raw}': expected 0..=3"
                    ));
                }
                m
            }
        };

        let cs_ref = node
            .get_attr("cs")
            .ok_or_else(|| format!("❌ [spi_component] '{id}' missing required attribute 'cs'"))?
            .trim_start_matches('$');
        // Numeric `cs="9"` (the `spi_example`s) is a pin number, not a gpio
        // id — accept it without an interner lookup. Otherwise it must point
        // at a known gpio peripheral.
        if cs_ref.parse::<u8>().is_err() {
            ctx.interner.get(cs_ref).ok_or_else(|| {
                format!("❌ [spi_component] '{id}' cs references undefined gpio '{cs_ref}'")
            })?;
        }

        let frequency = node.get_attr("frequency");

        let name = spi_ref.trim_start_matches('$');
        let handle = ctx
            .interner
            .get(name)
            .ok_or_else(|| format!("❌ [spi_component] '{id}' references undefined spi peripheral '{name}'"))?;
        let peripheral = ctx.active_peripherals[handle.0 as usize]
            .as_ref()
            .ok_or_else(|| format!("❌ [spi_component] '{id}' spi peripheral '{name}' is not active"))?;

        if !matches!(peripheral, ActivePeripheral::Spi) {
            return Err(format!(
                "❌ [spi_component] '{id}' expects an spi peripheral, got a non-SPI parent bus"
            ));
        }

        Ok(Self {
            id,
            spi_ref,
            mode,
            cs_ref,
            frequency,
            peripheral_handle: handle,
            peripheral: *peripheral,
        })
    }
}
